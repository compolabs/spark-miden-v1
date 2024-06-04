use miden_lib::notes::utils::build_p2id_recipient;
use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{Account, AccountCode, AccountId, AccountStorage, SlotItem, StorageSlot},
    assembly::{AssemblyContext, ModuleAst, ProgramAst},
    assets::{Asset, AssetVault, FungibleAsset},
    crypto::hash::rpo::RpoDigest,
    crypto::rand::{FeltRng, RpoRandomCoin},
    notes::{
        Note, NoteAssets, NoteDetails, NoteExecutionHint, NoteHeader, NoteInputs, NoteMetadata,
        NoteRecipient, NoteScript, NoteTag, NoteType,
    },
    transaction::TransactionArgs,
    vm::CodeBlock,
    Felt, NoteError, Word, ZERO,
};
use miden_processor::AdviceMap;
use miden_tx::TransactionExecutor;
use miden_vm::Assembler;

use crate::utils::{
    get_new_key_pair_with_advice_map, get_new_pk_and_authenticator, prove_and_verify_transaction,
    MockDataStore, ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN, ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1,
    ACCOUNT_ID_REGULAR_ACCOUNT_IMMUTABLE_CODE_ON_CHAIN, ACCOUNT_ID_SENDER, ACCOUNT_ID_SENDER_1,
};

const MASTS: [&str; 2] = [
    "0x61e28f7c3fd6d79ea2f225f8d64961ba935329b9a68016ada1fabf22eee726b0", // recieve_asset
    "0x74de7e94e5afc71e608f590c139ac51f446fc694da83f93d968b019d1d2b7306", // send_asset
];

const ACCOUNT_CODE: &str = include_str!("../../src/accounts/user_wallet.masm");

pub fn account_code(assembler: &Assembler) -> AccountCode {
    let account_module_ast = ModuleAst::parse(ACCOUNT_CODE).unwrap();
    let code = AccountCode::new(account_module_ast, assembler).unwrap();

    let current: [String; 2] = [code.procedures()[0].to_hex(), code.procedures()[1].to_hex()];

    assert!(current == MASTS, "UPDATE MAST ROOT: {:?};", current);

    code
}

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
        vec![],
    )
    .unwrap();

    let account_vault = match assets {
        Some(asset) => AssetVault::new(&[asset]).unwrap(),
        None => AssetVault::new(&[]).unwrap(),
    };

    Account::new(
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

fn create_initial_message_note<R: FeltRng>(
    sender_account_id: AccountId,
    target_account_id: AccountId,
    note_input: Felt,
    mut rng: R,
) -> Result<Note, NoteError> {
    let note_script = include_str!("../../src/notes/SWAPp.masm");

    let note_assembler = TransactionKernel::assembler().with_debug_mode(true);

    let script_ast = ProgramAst::parse(&note_script).unwrap();
    let (note_script, _) = new_note_script(script_ast, &note_assembler).unwrap();

    let inputs = NoteInputs::new(vec![note_input])?;

    let tag = NoteTag::from_account_id(target_account_id, NoteExecutionHint::Local)?;
    let serial_num = rng.draw_word();
    let aux = ZERO;
    let note_type = NoteType::OffChain;
    let metadata = NoteMetadata::new(sender_account_id, note_type, tag, aux)?;

    // empty vault
    let vault = NoteAssets::new(vec![])?;
    let recipient = NoteRecipient::new(serial_num, note_script, inputs);

    Ok(Note::new(vault, metadata, recipient))
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

pub fn create_partial_swap_note<R: FeltRng>(
    sender: AccountId,
    offered_asset: Asset,
    requested_asset: Asset,
    note_type: NoteType,
    mut rng: R,
) -> Result<(Note, NoteDetails, RpoDigest), NoteError> {
    let note_code = include_str!("../../src/notes/SWAPp.masm");
    let (note_script, _code_block) = new_note_script(
        ProgramAst::parse(note_code).unwrap(),
        &TransactionKernel::assembler().with_debug_mode(true),
    )
    .unwrap();

    let payback_serial_num = rng.draw_word();
    let payback_recipient = build_p2id_recipient(sender, payback_serial_num)?;

    let payback_recipient_word: Word = payback_recipient.digest().into();
    let requested_asset_word: Word = requested_asset.into();
    let payback_tag = NoteTag::from_account_id(sender, NoteExecutionHint::Local)?;

    let inputs = NoteInputs::new(vec![
        payback_recipient_word[0],
        payback_recipient_word[1],
        payback_recipient_word[2],
        payback_recipient_word[3],
        requested_asset_word[0],
        requested_asset_word[1],
        requested_asset_word[2],
        requested_asset_word[3],
        payback_tag.inner().into(),
    ])?;

    // build the tag for the SWAP use case
    let tag = build_swap_tag(note_type, &offered_asset, &requested_asset)?;
    let serial_num = rng.draw_word();
    let aux = ZERO;

    // build the outgoing note
    let metadata = NoteMetadata::new(sender, note_type, tag, aux)?;
    let assets = NoteAssets::new(vec![offered_asset])?;
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), inputs);
    let note = Note::new(assets, metadata, recipient);

    // build the payback note details
    let payback_assets = NoteAssets::new(vec![requested_asset])?;
    let payback_note = NoteDetails::new(payback_assets, payback_recipient);

    let note_script_hash = note_script.hash();

    Ok((note, payback_note, note_script_hash))
}

pub fn create_output_note(note_input: Option<Felt>) -> Result<(Note, RpoDigest), NoteError> {
    let sender_account_id: AccountId =
        AccountId::try_from(ACCOUNT_ID_REGULAR_ACCOUNT_IMMUTABLE_CODE_ON_CHAIN).unwrap();

    // Create target smart contract
    let target_account_id =
        AccountId::try_from(ACCOUNT_ID_REGULAR_ACCOUNT_IMMUTABLE_CODE_ON_CHAIN).unwrap();

    let note_assembler = TransactionKernel::assembler().with_debug_mode(true);

    let note_script = include_str!("../../src/notes/SWAPp.masm");

    let script_ast = ProgramAst::parse(&note_script).unwrap();
    let (note_script, _) = new_note_script(script_ast, &note_assembler).unwrap();

    // add the inputs to the note
    let input_values = match note_input {
        Some(value) => vec![value],
        None => vec![],
    };

    let inputs: NoteInputs = NoteInputs::new(input_values).unwrap();

    let tag = NoteTag::from_account_id(target_account_id, NoteExecutionHint::Local).unwrap();
    let serial_num = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];
    let aux = ZERO;
    let note_type = NoteType::OffChain;
    let metadata = NoteMetadata::new(sender_account_id, note_type, tag, aux).unwrap();

    // empty vault
    let vault: NoteAssets = NoteAssets::new(vec![]).unwrap();
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), inputs);

    let note_script_hash = note_script.hash();

    Ok((Note::new(vault, metadata, recipient), note_script_hash))
}

/* // Run this first to check MASTs are correct
#[test]
pub fn check_account_masts() {
    let assembler: Assembler = TransactionKernel::assembler().with_debug_mode(true);

    let account_module_ast = ModuleAst::parse(ACCOUNT_CODE).unwrap();
    let code = AccountCode::new(account_module_ast, &assembler).unwrap();

    let current: [String; 2] = [code.procedures()[0].to_hex(), code.procedures()[1].to_hex()];
    println!("{:?}", current);
    assert!(current == MASTS, "UPDATE MAST ROOT: {:?};", current);
} */

#[test]
pub fn get_note_script_hash() {
    // SWAPp note creator
    let sender_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // Offered Asset
    let faucet_id = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let offered_asset: Asset = FungibleAsset::new(faucet_id, 100).unwrap().into();

    // Requested Asset
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let requested_asset: Asset = FungibleAsset::new(faucet_id_2, 100).unwrap().into();

    let (swap_note, _payback_note, note_script_hash) = create_partial_swap_note(
        sender_account_id,
        offered_asset,
        requested_asset,
        NoteType::Public,
        RpoRandomCoin::new([Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)]),
    )
    .unwrap();

    let tag = swap_note.clone().metadata().tag();
    let note_type = swap_note.clone().metadata().note_type();

    println!("{:?}", tag);
    println!("{:?}", note_type);
    println!("Note script hash: {:?}", note_script_hash);
}

#[test]
fn test_partial_swap_fill() {
    // ASSETS
    // Offered Asset
    let faucet_id = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let amount_token_a = 100;
    let token_a: Asset = FungibleAsset::new(faucet_id, amount_token_a)
        .unwrap()
        .into();

    // Requested Asset
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let amount_token_b = 100;
    let token_b: Asset = FungibleAsset::new(faucet_id_2, amount_token_b)
        .unwrap()
        .into();

    // ACCOUNT IDs
    // SWAPp note creator
    let swapp_creator_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // SWAPp note consumer
    let swapp_consumer_account_id = AccountId::try_from(ACCOUNT_ID_SENDER_1).unwrap();

    // SWAPp note consumer wallet balance
    let swap_consumer_balance_token_b = 80;
    let swap_consumer_token_b = FungibleAsset::new(faucet_id_2, swap_consumer_balance_token_b)
        .unwrap()
        .into();
    let (target_pub_key, _target_falcon_auth) = get_new_pk_and_authenticator();

    // SWAPp note consumer wallet
    let swap_consumer_wallet = get_custom_account_code(
        swapp_consumer_account_id,
        target_pub_key,
        Some(swap_consumer_token_b),
    );

    // SWAPp note
    let (swap_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id,
        token_a,
        token_b,
        NoteType::Public,
        RpoRandomCoin::new([Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)]),
    )
    .unwrap();

    // CONSTRUCT AND EXECUTE TX (Success)
    // --------------------------------------------------------------------------------------------
    let data_store = MockDataStore::with_existing(
        Some(swap_consumer_wallet.clone()),
        Some(vec![swap_note.clone()]),
    );

    let mut executor: TransactionExecutor<_, ()> =
        TransactionExecutor::new(data_store.clone(), None).with_debug_mode(true);
    executor.load_account(swapp_consumer_account_id).unwrap();

    let block_ref = data_store.block_header.block_num();
    let note_ids = data_store
        .notes
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
    let executed_transaction = executor
        .execute_transaction(
            swapp_consumer_account_id,
            block_ref,
            &note_ids,
            tx_args_target.clone(),
        )
        .unwrap();

    // Note outputted by the transaction
    let tx_output_note = executed_transaction.output_notes().get_note(0);

    // Note expected to be outputted by the transaction
    let (_expected_note, _note_script_hash) = create_output_note(Some(Felt::new(301))).unwrap();

    assert_eq!(executed_transaction.output_notes().num_notes(), 2);

    /*
    // Check that the output note is the same as the expected note
    assert_eq!(
        NoteHeader::from(tx_output_note).metadata(),
        NoteHeader::from(expected_note.clone()).metadata()
    );
    assert_eq!(
        NoteHeader::from(tx_output_note),
        NoteHeader::from(expected_note.clone())
    );

    // comment out to speed up test
    // assert!(prove_and_verify_transaction(executed_transaction.clone()).is_ok());

    // CONSTRUCT AND EXECUTE TX 2 (Success)
    // --------------------------------------------------------------------------------------------
    let data_store_1 = MockDataStore::with_existing(
        Some(target_account.clone()),
        Some(vec![expected_note.clone()]),
    );

    let mut executor: TransactionExecutor<_, ()> =
        TransactionExecutor::new(data_store_1.clone(), None).with_debug_mode(true);
    executor.load_account(target_account_id).unwrap();

    let block_ref = data_store_1.block_header.block_num();
    let note_ids_1 = data_store_1
        .notes
        .iter()
        .map(|expected_note| expected_note.id())
        .collect::<Vec<_>>();

    // Execute the transaction and get the witness
    let executed_transaction_1 = executor
        .execute_transaction(
            target_account_id,
            block_ref,
            &note_ids_1,
            tx_args_target.clone(),
        )
        .unwrap(); */

    // commented out to speed up test
    // assert!(prove_and_verify_transaction(executed_transaction_1.clone()).is_ok());
}
