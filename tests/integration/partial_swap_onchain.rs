use std::collections::BTreeMap;

use miden_client::{
    accounts::AccountTemplate, transactions::transaction_request::TransactionRequest,
    utils::Serializable,
};
use miden_lib::notes::utils::build_p2id_recipient;
use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{AccountId, AccountStorageType, AuthSecretKey},
    assembly::{AssemblyContext, ModuleAst, ProgramAst},
    assets::{Asset, FungibleAsset, TokenSymbol},
    crypto::hash::rpo::RpoDigest,
    crypto::rand::{FeltRng, RpoRandomCoin},
    notes::{
        Note, NoteAssets, NoteDetails, NoteExecutionHint, NoteHeader, NoteInputs, NoteMetadata,
        NoteRecipient, NoteScript, NoteTag, NoteType,
    },
    vm::CodeBlock,
    Felt, NoteError, Word, ZERO,
};
use miden_vm::Assembler;

use super::common::*;

// CUSTOM TRANSACTION REQUEST
// ================================================================================================
//
// The following functions are for testing custom transaction code. What the test does is:
//
// - Create a custom tx that mints a custom note which checks that the note args are as expected
//   (ie, a word of 4 felts that represent [9, 12, 18, 3])
//
// - Create another transaction that consumes this note with custom code. This custom code only
//   asserts that the {asserted_value} parameter is 0. To test this we first execute with
//   an incorrect value passed in, and after that we try again with the correct value.
//
// Because it's currently not possible to create/consume notes without assets, the P2ID code
// is used as the base for the note code.
#[tokio::test]
async fn test_partial_swap_fill() {
    let mut client = create_test_client();
    wait_for_node(&mut client).await;

    let account_template = AccountTemplate::BasicWallet {
        mutable_code: false,
        storage_type: AccountStorageType::OffChain,
    };

    client.sync_state().await.unwrap();
    // Insert Account
    let (regular_account, _seed) = client.new_account(account_template).unwrap();

    let account_template = AccountTemplate::FungibleFaucet {
        token_symbol: TokenSymbol::new("TEST").unwrap(),
        decimals: 5u8,
        max_supply: 10_000u64,
        storage_type: AccountStorageType::OffChain,
    };
    let (fungible_faucet, _seed) = client.new_account(account_template).unwrap();

    // Execute mint transaction in order to create custom note
    // let note = mint_custom_note(&mut client, fungible_faucet.id(), regular_account.id()).await;

    let swap_note = create_partial_swap_note(
        regular_account.id(),
        regular_account.id(),
        FungibleAsset::new(fungible_faucet.id(), 10).unwrap().into(),
        FungibleAsset::new(fungible_faucet.id(), 10).unwrap().into(),
        NoteType::Public,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
    )
    .await
    .unwrap();

    client.sync_state().await.unwrap();

    // Prepare transaction

    // SWAPp note args
    let note_args: [[Felt; 4]; 1] = [[Felt::new(0), Felt::new(0), Felt::new(0), Felt::new(0)]];
    let note_args_map = BTreeMap::from([(swap_note.id(), Some(note_args[0]))]);

    let account_wallet = "
        use.miden::contracts::wallets::basic->basic_wallet
        use.miden::contracts::auth::basic->basic_eoa

        export.basic_wallet::receive_asset
        export.basic_wallet::send_asset
        export.basic_eoa::auth_tx_rpo_falcon512
    ";

    // SUCCESS EXECUTION

    // let success_code = code.replace("{asserted_value}", "0");
    let program = ProgramAst::parse(&account_wallet).unwrap();

    let tx_script = {
        let account_auth = client.get_account_auth(regular_account.id()).unwrap();
        let (pubkey_input, advice_map): (Word, Vec<Felt>) = match account_auth {
            AuthSecretKey::RpoFalcon512(key) => (
                key.public_key().into(),
                key.to_bytes()
                    .iter()
                    .map(|a| Felt::new(*a as u64))
                    .collect::<Vec<Felt>>(),
            ),
        };

        let script_inputs = vec![(pubkey_input, advice_map)];
        client
            .compile_tx_script(program, script_inputs, vec![])
            .unwrap()
    };

    let transaction_request = TransactionRequest::new(
        regular_account.id(),
        note_args_map,
        vec![],
        vec![],
        Some(tx_script),
    );

    execute_tx_and_sync(&mut client, transaction_request).await;

    client.sync_state().await.unwrap();
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

async fn create_partial_swap_note(
    sender: AccountId,
    last_consumer: AccountId,
    offered_asset: Asset,
    requested_asset: Asset,
    note_type: NoteType,
    serial_num: [Felt; 4],
) -> Result<(Note), NoteError> {
    let note_code = include_str!("../../src/notes/SWAPp.masm");
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
    // let payback_assets = NoteAssets::new(vec![requested_asset])?;
    // let payback_note = NoteDetails::new(payback_assets, payback_recipient);
    // let note_script_hash = note_script.hash();

    Ok(note)
}
