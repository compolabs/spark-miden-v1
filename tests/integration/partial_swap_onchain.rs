use std::collections::BTreeMap;

use miden_client::{
    transactions::transaction_request::TransactionRequest,
    utils::Serializable,
};
use miden_lib::notes::{utils::build_p2id_recipient, create_p2id_note};
use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{AccountId, AuthSecretKey},
    assembly::{AssemblyContext, ProgramAst},
    assets::{Asset, FungibleAsset},
    crypto::hash::rpo::Rpo256,
    notes::{
        Note, NoteAssets, NoteExecutionHint, NoteInputs, NoteMetadata, NoteRecipient, NoteScript,
        NoteTag, NoteType,
    },
    vm::{CodeBlock, AdviceMap},
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

    println!("Creating accounts and tokens...");

    // Set up accounts and tokens
    let (account_a, account_b, asset_a_account, asset_b_account) =
        setup_with_tokens(&mut client).await;

    println!("Setup completed");

    let asset_a_amount: u64 = format_value_with_decimals(100, 6);
    let asset_b_amount: u64 = format_value_with_decimals(100, 6);
    let asset_b_amount_in: u64 = format_value_with_decimals(80, 6);

    // mint Asset A in Account A
    let note = mint_note_with_amount(
        &mut client,
        account_a.id(),
        asset_a_account.id(),
        asset_a_amount,
        NoteType::OffChain,
    )
    .await;
    consume_notes(&mut client, account_a.id(), &[note]).await;
    assert_account_has_single_asset(
        &client,
        account_a.id(),
        asset_a_account.id(),
        asset_a_amount,
    ).await;

    // mint Asset B in Account B
    let note = mint_note_with_amount(
        &mut client,
        account_b.id(),
        asset_b_account.id(),
        asset_b_amount_in,
        NoteType::OffChain,
    )
    .await;
    consume_notes(&mut client, account_b.id(), &[note]).await;
    assert_account_has_single_asset(
        &client,
        account_b.id(),
        asset_b_account.id(),
        asset_b_amount_in,
    ).await;

    println!("MINT NOTES CREATED");

    // Create a SWAPp note using account A
    let swap_note = create_partial_swap_note(
        &mut client,
        account_a.id(),
        account_a.id(),
        asset_a_account.id(),
        asset_a_amount,
        asset_b_account.id(),
        asset_b_amount,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
    )
    .await
    .unwrap();

    client.sync_state().await.unwrap();

    println!("Swap note created. SwapID: {:?}", swap_note.id());
    
    // Prepare the transaction to consume the SWAPp note
    const NOTE_ARGS: [Felt; 8] = [
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
    ];
    let note_args_commitment = Rpo256::hash_elements(&NOTE_ARGS);

    let note_args_map = BTreeMap::from([(swap_note.id(), Some(note_args_commitment.into()))]);
    let mut advice_map = AdviceMap::new();
    advice_map.insert(note_args_commitment, NOTE_ARGS.to_vec());

    let tx_script = "
        use.miden::contracts::auth::basic->auth_tx

        begin
            call.auth_tx::auth_tx_rpo_falcon512
        end
    ";

    let program = ProgramAst::parse(&tx_script).unwrap();

    // Compile transaction script for account B
    let tx_script = {
        let account_auth = client.get_account_auth(account_b.id()).unwrap();
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

    // SWAPp note Oupput 
    let amount_a_out = calculate_tokens_a_for_b(asset_a_amount, asset_b_amount, asset_b_amount_in);
    let amount_a_remaining: u64 = asset_a_amount - amount_a_out;
    let amount_b_requested_remaining: u64 = asset_b_amount - asset_b_amount_in;


    // let bal = swap_note.assets();
    println!("SWAPp note assets: {:?}", swap_note.assets());
    println!("AccountB asset B amount: {:?}", account_b.vault().get_balance(asset_b_account.id()));
    println!("AccountB asset A amount: {:?}", account_b.vault().get_balance(asset_a_account.id()));
    println!("faucetID A: {:?}", asset_a_account.id());
    println!("faucetID B: {:?}", asset_b_account.id());

    
    // let ouput_p2id = create_p2id_note(account_b, account_a, assets, note_type, aux, rng)

    // Define the transaction request for account B to consume the note
    let transaction_request = TransactionRequest::new(
        account_b.id(),
        vec![],
        note_args_map,
        vec![],
        vec![],
        Some(tx_script),
        Some(advice_map.clone()),
    )
    .unwrap();

    // Execute the transaction
    execute_tx_and_sync(&mut client, transaction_request).await;

    // Ensure synchronization of client state
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
    client: &mut TestClient,
    sender: AccountId,
    last_consumer: AccountId,
    offered_asset_id: AccountId,
    offered_asset_amount: u64,
    requested_asset_id: AccountId,
    requested_asset_amount: u64,
    note_type: NoteType,
    serial_num: [Felt; 4],
) -> Result<Note, NoteError> {
    let note_code = include_str!("../../src/notes/SWAPp.masm");
    let (note_script, _code_block) = new_note_script(
        ProgramAst::parse(note_code).unwrap(),
        &TransactionKernel::assembler(),
    )
    .unwrap();

    let offered_asset: Asset =
        Asset::from(FungibleAsset::new(offered_asset_id, offered_asset_amount).unwrap());
    let requested_asset: Asset =
        Asset::from(FungibleAsset::new(requested_asset_id, requested_asset_amount).unwrap());

    let payback_recipient = build_p2id_recipient(sender, serial_num)?;

    let payback_recipient_word: Word = payback_recipient.digest().into();
    let requested_asset_word: Word = requested_asset.into();

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
    let swap_note = Note::new(assets.clone(), metadata, recipient.clone());

    println!("Attempting to mint SWAPp note...");

    // note created, now let's mint it
    let recipient = swap_note
        .recipient()
        .digest()
        .iter()
        .map(|x| x.as_int().to_string())
        .collect::<Vec<_>>()
        .join(".");

    let code = "
    use.miden::contracts::auth::basic->auth_tx
    use.miden::contracts::wallets::basic->wallet
    use.std::sys

    begin
        push.{recipient}
        push.2
        push.0
        push.{tag}
        push.{amount}
        push.0.0
        push.{token_id}

        call.wallet::send_asset

        call.auth_tx::auth_tx_rpo_falcon512

        exec.sys::truncate_stack
    end
    "
    .replace("{recipient}", &recipient.clone())
    .replace("{tag}", &Felt::new(tag.clone().into()).to_string())
    .replace(
        "{amount}",
        &offered_asset.unwrap_fungible().amount().to_string(),
    )
    .replace(
        "{token_id}",
        &Felt::new(offered_asset.faucet_id().into()).to_string(),
    );

    let program = ProgramAst::parse(&code).unwrap();
    let tx_script = client.compile_tx_script(program, vec![], vec![]).unwrap();

    let transaction_request = TransactionRequest::new(
        sender,
        vec![],
        BTreeMap::new(),
        vec![swap_note.clone()],
        vec![],
        Some(tx_script),
        None,
    )
    .unwrap();

    println!("Attempting to create SWAPp note...");
    println!("SWAPp noteID {:?}", swap_note.id());

    let _ = execute_tx_and_sync(client, transaction_request).await;

    println!("SWAPp note created!");

    Ok(swap_note)
}
