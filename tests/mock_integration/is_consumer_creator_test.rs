use miden_lib::notes::utils::build_p2id_recipient;
use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{Account, AccountCode, AccountId, AccountStorage, SlotItem, StorageSlot},
    assembly::{AssemblyContext, ModuleAst, ProgramAst},
    assets::{Asset, AssetVault, FungibleAsset},
    crypto::hash::rpo::RpoDigest,
    notes::{
        Note, NoteAssets, NoteDetails, NoteExecutionHint, NoteInputs, NoteMetadata, NoteRecipient,
        NoteScript, NoteTag, NoteType,
    },
    transaction::TransactionArgs,
    vm::CodeBlock,
    Felt, NoteError, Word, ZERO,
};
use miden_processor::AdviceMap;
use miden_tx::{testing::TransactionContextBuilder, TransactionExecutor};
use miden_vm::Assembler;
use std::collections::BTreeMap;

use crate::utils::{
    get_new_pk_and_authenticator, /* prove_and_verify_transaction */
    ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN, ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1, ACCOUNT_ID_SENDER,
    ACCOUNT_ID_SENDER_1,
};

pub fn get_custom_account_code(
    account_id: AccountId,
    public_key: Word,
    assets: Option<Asset>,
) -> Account {
    let account_code_src = include_str!("../../src/accounts/user_wallet.masm");
    let account_code_ast = ModuleAst::parse(account_code_src).unwrap();
    let account_assembler = TransactionKernel::assembler().with_debug_mode(true);

    let account_code = AccountCode::new(account_code_ast.clone(), &account_assembler).unwrap();

    let account_storage = AccountStorage::new(
        vec![SlotItem {
            index: 0,
            slot: StorageSlot::new_value(public_key),
        }],
        BTreeMap::new(),
    )
    .unwrap();

    let account_vault = match assets {
        Some(asset) => AssetVault::new(&[asset]).unwrap(),
        None => AssetVault::new(&[]).unwrap(),
    };

    Account::from_parts(
        account_id,
        account_vault,
        account_storage,
        account_code,
        Felt::new(1),
    )
}

pub fn new_note_script(
    code: ProgramAst,
    assembler: &Assembler,
) -> Result<(NoteScript, CodeBlock), NoteError> {
    // Compile the code in the context with phantom calls enabled
    let code_block = assembler
        .compile_in_context(
            &code,
            &mut AssemblyContext::for_program(Some(&code)).with_phantom_calls(true),
        )
        .map_err(NoteError::ScriptCompilationError)?;

    // Use the from_parts method to create a NoteScript instance
    let note_script = NoteScript::from_parts(code, code_block.hash());

    Ok((note_script, code_block))
}

fn build_swap_tag(
    note_type: NoteType,
    offered_asset: &Asset,
    requested_asset: &Asset,
) -> Result<NoteTag, NoteError> {
    const SWAP_USE_CASE_ID: u16 = 0;

    // get bits 4..12 from faucet IDs of both assets, these bits will form the tag payload; the
    // reason we skip the 4 most significant bits is that these encode metadata of underlying
    // faucets and are likely to be the same for many different faucets.

    let offered_asset_id: u64 = offered_asset.faucet_id().into();
    let offered_asset_tag = (offered_asset_id >> 52) as u8;

    let requested_asset_id: u64 = requested_asset.faucet_id().into();
    let requested_asset_tag = (requested_asset_id >> 52) as u8;

    let payload = ((offered_asset_tag as u16) << 8) | (requested_asset_tag as u16);

    let execution = NoteExecutionHint::Local;
    match note_type {
        NoteType::Public => NoteTag::for_public_use_case(SWAP_USE_CASE_ID, payload, execution),
        _ => NoteTag::for_local_use_case(SWAP_USE_CASE_ID, payload),
    }
}

pub fn create_partial_swap_note(
    sender: AccountId,
    last_consumer: AccountId,
    offered_asset: Asset,
    requested_asset: Asset,
    note_type: NoteType,
    serial_num: [Felt; 4],
) -> Result<(Note, NoteDetails, RpoDigest), NoteError> {
    let note_code = include_str!("../../src/test/is_consumer_creator.masm");
    let (note_script, _code_block) = new_note_script(
        ProgramAst::parse(note_code).unwrap(),
        &TransactionKernel::assembler().with_debug_mode(true),
    )
    .unwrap();

    let payback_recipient = build_p2id_recipient(sender, serial_num)?;

    let payback_recipient_word: Word = payback_recipient.digest().into();
    let requested_asset_word: Word = requested_asset.into();
    // let payback_tag = NoteTag::from_account_id(sender, NoteExecutionHint::Local)?;

    // build the tag for the SWAP use case
    let tag = build_swap_tag(note_type, &offered_asset, &requested_asset)?;

    let inputs = NoteInputs::new(vec![
        payback_recipient_word[0],
        payback_recipient_word[1],
        payback_recipient_word[2],
        payback_recipient_word[3],
        requested_asset_word[0],
        requested_asset_word[1],
        requested_asset_word[2],
        requested_asset_word[3],
        tag.inner().into(),
    ])?;

    let aux = ZERO;

    // build the outgoing note
    let metadata = NoteMetadata::new(last_consumer, note_type, tag, aux)?;
    let assets = NoteAssets::new(vec![offered_asset])?;
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), inputs.clone());
    let note = Note::new(assets.clone(), metadata, recipient.clone());

    // build the payback note details
    let payback_assets = NoteAssets::new(vec![requested_asset])?;
    let payback_note = NoteDetails::new(payback_assets, payback_recipient);

    let note_script_hash = note_script.hash();

    Ok((note, payback_note, note_script_hash))
}

fn format_value_with_decimals(value: u64, decimals: u32) -> u64 {
    value * 10u64.pow(decimals)
}

// @dev Test that the is_consumer_is_creator procedure succeeds when the consumer is not the creator
#[test]
fn test_is_consumer_creator_success() {
    // ASSETS
    // Offered Asset
    let faucet_id_1 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let amount_token_a: u64 = format_value_with_decimals(607, 6);
    let offered_token_a: Asset = FungibleAsset::new(faucet_id_1, amount_token_a)
        .unwrap()
        .into();

    // Requested Asset
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let requested_amount_token_b = format_value_with_decimals(987, 6);
    let requested_token_b: Asset = FungibleAsset::new(faucet_id_2, requested_amount_token_b)
        .unwrap()
        .into();

    // ACCOUNT IDs
    // SWAPp note creator
    let swapp_creator_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // SWAPp note consumer
    let swapp_consumer_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // SWAPp note consumer wallet balance
    let token_b_amount_in = format_value_with_decimals(387, 6);
    let swap_consumer_token_b = FungibleAsset::new(faucet_id_2, token_b_amount_in)
        .unwrap()
        .into();
    let (target_pub_key, target_falcon_auth) = get_new_pk_and_authenticator();

    // SWAPp note consumer wallet
    let swap_consumer_wallet = get_custom_account_code(
        swapp_consumer_account_id,
        target_pub_key,
        Some(swap_consumer_token_b),
    );

    // SWAPp note
    let (swap_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id.clone(),
        swapp_creator_account_id.clone(),
        offered_token_a,
        requested_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
    )
    .unwrap();

    // CONSTRUCT AND EXECUTE TX (Success)
    // --------------------------------------------------------------------------------------------
    let tx_context = TransactionContextBuilder::new(swap_consumer_wallet.clone())
        .input_notes(vec![swap_note.clone()])
        .build();

    let mut executor =
        TransactionExecutor::new(tx_context.clone(), Some(target_falcon_auth.clone()))
            .with_debug_mode(true);
    executor.load_account(swapp_consumer_account_id).unwrap();

    let block_ref = tx_context.tx_inputs().block_header().block_num();
    let note_ids = tx_context
        .tx_inputs()
        .input_notes()
        .iter()
        .map(|note| note.id())
        .collect::<Vec<_>>();

    let tx_script_code = include_str!("../../src/tx_scripts/tx_script.masm");
    let tx_script_ast = ProgramAst::parse(tx_script_code).unwrap();

    let tx_script_target = executor
        .compile_tx_script(tx_script_ast.clone(), vec![], vec![])
        .unwrap();

    let tx_args_target = TransactionArgs::new(Some(tx_script_target), None, AdviceMap::default());

    // Execute the transaction and get the witness
    let executed_transaction = executor.execute_transaction(
        swapp_consumer_account_id,
        block_ref,
        &note_ids,
        tx_args_target.clone(),
    );
    assert!(executed_transaction.is_ok());
}

// @dev Test that the is_consumer_is_creator procedure fails when the consumer is not the creator
#[test]
fn test_is_consumer_creator_unauthorized() {
    // ASSETS
    // Offered Asset
    let faucet_id_1 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let amount_token_a: u64 = format_value_with_decimals(607, 6);
    let offered_token_a: Asset = FungibleAsset::new(faucet_id_1, amount_token_a)
        .unwrap()
        .into();

    // Requested Asset
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let requested_amount_token_b = format_value_with_decimals(987, 6);
    let requested_token_b: Asset = FungibleAsset::new(faucet_id_2, requested_amount_token_b)
        .unwrap()
        .into();

    // ACCOUNT IDs
    // SWAPp note creator
    let swapp_creator_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // SWAPp note consumer
    let swapp_consumer_account_id = AccountId::try_from(ACCOUNT_ID_SENDER_1).unwrap();

    // SWAPp note consumer wallet balance
    let token_b_amount_in = format_value_with_decimals(387, 6);
    let swap_consumer_token_b = FungibleAsset::new(faucet_id_2, token_b_amount_in)
        .unwrap()
        .into();
    let (target_pub_key, target_falcon_auth) = get_new_pk_and_authenticator();

    // SWAPp note consumer wallet
    let swap_consumer_wallet = get_custom_account_code(
        swapp_consumer_account_id,
        target_pub_key,
        Some(swap_consumer_token_b),
    );

    // SWAPp note
    let (swap_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id.clone(),
        swapp_creator_account_id.clone(),
        offered_token_a,
        requested_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
    )
    .unwrap();

    // CONSTRUCT AND EXECUTE TX (Success)
    // --------------------------------------------------------------------------------------------
    let tx_context = TransactionContextBuilder::new(swap_consumer_wallet.clone())
        .input_notes(vec![swap_note.clone()])
        .build();

    let mut executor =
        TransactionExecutor::new(tx_context.clone(), Some(target_falcon_auth.clone()))
            .with_debug_mode(true);
    executor.load_account(swapp_consumer_account_id).unwrap();

    let block_ref = tx_context.tx_inputs().block_header().block_num();
    let note_ids = tx_context
        .tx_inputs()
        .input_notes()
        .iter()
        .map(|note| note.id())
        .collect::<Vec<_>>();

    let tx_script_code = include_str!("../../src/tx_scripts/tx_script.masm");
    let tx_script_ast = ProgramAst::parse(tx_script_code).unwrap();

    let tx_script_target = executor
        .compile_tx_script(tx_script_ast.clone(), vec![], vec![])
        .unwrap();

    let tx_args_target = TransactionArgs::new(Some(tx_script_target), None, AdviceMap::default());

    // Execute the transaction and get the witness
    let executed_transaction = executor.execute_transaction(
        swapp_consumer_account_id,
        block_ref,
        &note_ids,
        tx_args_target.clone(),
    );

    assert!(executed_transaction.is_err());
}
