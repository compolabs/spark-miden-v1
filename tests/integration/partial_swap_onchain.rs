use std::collections::BTreeMap;

use miden_client::{transactions::transaction_request::TransactionRequest, utils::Serializable};
use miden_objects::{
    accounts::AuthSecretKey,
    assembly::{AssemblyContext, ProgramAst},
    assets::{Asset, FungibleAsset},
    crypto::hash::rpo::Rpo256,
    notes::{NoteScript, NoteType},
    vm::{AdviceMap, CodeBlock},
    Felt, NoteError, Word,
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
    )
    .await;

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
    )
    .await;

    client.sync_state().await.unwrap();

    println!("MINT NOTES CREATED");

    // @dev TODO create P2ID and SWAPp serial nums
    let serial_num = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];

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
        serial_num,
    )
    .await
    .unwrap();

    client.sync_state().await.unwrap();

    println!("Swap note created");

    // Prepare the transaction to consume the SWAPp note
    const NOTE_ARGS: [Felt; 4] = [Felt::new(100), Felt::new(0), Felt::new(0), Felt::new(0)];
    let note_args_commitment = Rpo256::hash_elements(&NOTE_ARGS);

    let note_args_map = BTreeMap::from([(swap_note.id(), Some(NOTE_ARGS))]);
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

    let expected_swap_note_output = create_partial_swap_note_offchain(
        account_a.id(),
        account_b.id(),
        asset_a_account.id(),
        amount_a_remaining,
        asset_b_account.id(),
        amount_b_requested_remaining,
        NoteType::OffChain,
        serial_num.clone(),
    )
    .unwrap();

    let p2id_assets: Vec<Asset> = vec![Asset::from(
        FungibleAsset::new(asset_b_account.id(), asset_b_amount_in).unwrap(),
    )];

    let expected_p2id_output = create_p2id_note_with_serial_num(
        account_b.id(),
        account_a.id(),
        p2id_assets,
        NoteType::OffChain,
        Felt::new(0),
        serial_num,
    )
    .unwrap();

    println!("____________");
    println!("Account A id: {:?}", account_a.id());
    println!("Account B id: {:?}", account_b.id());

    println!("Asset A id: {:?}", asset_a_account.id());
    println!("Asset B id: {:?}", asset_b_account.id());

    // let bal = swap_note.assets();
    println!("SWAPp note assets: {:?}", swap_note.assets());

    // consuming SWAPp note with account b
    println!("calling account balance for user B");
    print_account_balance(&client, account_b.id()).await;

    assert_eq!(swap_note.metadata().sender(), account_a.id());
    println!("SWAP note sender {:?}", swap_note.metadata().sender());

    // Define the transaction request for account B to consume the note
    let transaction_request = TransactionRequest::new(
        account_b.id(),
        vec![],
        note_args_map,
        vec![expected_p2id_output, expected_swap_note_output],
        vec![],
        Some(tx_script),
        Some(advice_map.clone()),
    )
    .unwrap();

    println!("calling execute tx");

    client.sync_state().await.unwrap();

    // Execute the transaction
    execute_tx_and_sync(&mut client, transaction_request).await;

    // Ensure synchronization of client state
    client.sync_state().await.unwrap();

    // consuming SWAPp note with account b
    println!("calling account balance for user B");
    print_account_balance(&client, account_b.id()).await;
}
