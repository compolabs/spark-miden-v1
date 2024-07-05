use std::collections::BTreeMap;

use miden_lib::notes::utils::build_p2id_recipient;
use miden_lib::transaction::TransactionKernel;
use miden_client::{
    accounts::AccountTemplate, transactions::transaction_request::TransactionRequest,
    utils::Serializable,
};
use miden_objects::{
    accounts::{AccountId, AccountStorageType, AuthSecretKey},
    assembly::{AssemblyContext, ModuleAst, ProgramAst}, 
       assets::{Asset, FungibleAsset, TokenSymbol},
    crypto::rand::{FeltRng, RpoRandomCoin},
    crypto::hash::rpo::RpoDigest,
    notes::{
        Note, NoteAssets, NoteDetails, NoteExecutionHint, NoteHeader, NoteInputs, NoteMetadata,
        NoteRecipient, NoteScript, NoteTag, NoteType,
    },
    vm::CodeBlock,
    Felt, Word, NoteError, ZERO
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
    let note = mint_custom_note(&mut client, fungible_faucet.id(), regular_account.id()).await;
    client.sync_state().await.unwrap();

    let swap_note = create_partial_swap_note(
        regular_account.id(),
        regular_account.id(),
        FungibleAsset::new(fungible_faucet.id(), 10).unwrap().into(),
        FungibleAsset::new(fungible_faucet.id(), 10).unwrap().into(),
        NoteType::Public,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
    ).await;

    println!("swap_note: {:?}", swap_note);

    // Prepare transaction

    // If these args were to be modified, the transaction would fail because the note code expects
    // these exact arguments
    let note_args = [[Felt::new(9), Felt::new(12), Felt::new(18), Felt::new(3)]];

    let note_args_map = BTreeMap::from([(note.id(), Some(note_args[0]))]);

    let code = "
        use.miden::contracts::auth::basic->auth_tx
        use.miden::kernels::tx::prologue
        use.miden::kernels::tx::memory

        begin
            push.0 push.{asserted_value}
            # => [0, {asserted_value}]
            assert_eq

            call.auth_tx::auth_tx_rpo_falcon512
        end
        ";

    /*     
    // SUCCESS EXECUTION

    let success_code = code.replace("{asserted_value}", "0");
    let program = ProgramAst::parse(&success_code).unwrap();

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
    */
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
) -> Result<(Note, NoteDetails, RpoDigest), NoteError> {
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
    let payback_assets = NoteAssets::new(vec![requested_asset])?;
    let payback_note = NoteDetails::new(payback_assets, payback_recipient);

    let note_script_hash = note_script.hash();

    Ok((note, payback_note, note_script_hash))
}

async fn mint_custom_note(
    client: &mut TestClient,
    faucet_account_id: AccountId,
    target_account_id: AccountId,
) -> Note {
    // Prepare transaction
    let mut random_coin = RpoRandomCoin::new(Default::default());
    let note = create_custom_note(
        client,
        faucet_account_id,
        target_account_id,
        &mut random_coin,
    );

    let recipient = note
        .recipient()
        .digest()
        .iter()
        .map(|x| x.as_int().to_string())
        .collect::<Vec<_>>()
        .join(".");

    let note_tag = note.metadata().tag().inner();

    let code = "
    use.miden::contracts::faucets::basic_fungible->faucet
    use.miden::contracts::auth::basic->auth_tx
    
    begin
        push.{recipient}
        push.{note_type}
        push.0
        push.{tag}
        push.{amount}
        call.faucet::distribute
    
        call.auth_tx::auth_tx_rpo_falcon512
        dropw dropw
    end
    "
    .replace("{recipient}", &recipient)
    .replace(
        "{note_type}",
        &Felt::new(NoteType::OffChain as u64).to_string(),
    )
    .replace("{tag}", &Felt::new(note_tag.into()).to_string())
    .replace("{amount}", &Felt::new(10).to_string());

    let program = ProgramAst::parse(&code).unwrap();

    let tx_script = client.compile_tx_script(program, vec![], vec![]).unwrap();

    let transaction_request = TransactionRequest::new(
        faucet_account_id,
        BTreeMap::new(),
        vec![note.clone()],
        vec![],
        Some(tx_script),
    );

    let _ = execute_tx_and_sync(client, transaction_request).await;
    note
}

fn create_custom_note(
    client: &TestClient,
    faucet_account_id: AccountId,
    target_account_id: AccountId,
    rng: &mut RpoRandomCoin,
) -> Note {
    let expected_note_arg = [Felt::new(9), Felt::new(12), Felt::new(18), Felt::new(3)]
        .iter()
        .map(|x| x.as_int().to_string())
        .collect::<Vec<_>>()
        .join(".");

    let note_script =
        include_str!("asm/custom_p2id.masm").replace("{expected_note_arg}", &expected_note_arg);
    let note_script = ProgramAst::parse(&note_script).unwrap();
    let note_script = client.compile_note_script(note_script, vec![]).unwrap();

    let inputs = NoteInputs::new(vec![target_account_id.into()]).unwrap();
    let serial_num = rng.draw_word();
    let note_metadata = NoteMetadata::new(
        faucet_account_id,
        NoteType::OffChain,
        NoteTag::from_account_id(target_account_id, NoteExecutionHint::Local).unwrap(),
        Default::default(),
    )
    .unwrap();
    let note_assets = NoteAssets::new(vec![FungibleAsset::new(faucet_account_id, 10)
        .unwrap()
        .into()])
    .unwrap();
    let note_recipient = NoteRecipient::new(serial_num, note_script, inputs);
    Note::new(note_assets, note_metadata, note_recipient)
}
