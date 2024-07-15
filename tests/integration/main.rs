use miden_client::{
    accounts::AccountTemplate,
    notes::NoteRelevance,
    rpc::{AccountDetails, NodeRpcClient, TonicRpcClient},
    store::{InputNoteRecord, NoteFilter, NoteStatus, TransactionFilter},
    transactions::{
        transaction_request::{PaymentTransactionData, TransactionTemplate},
        TransactionExecutorError, TransactionStatus,
    },
    ClientError,
};
use miden_objects::{
    accounts::{AccountId, AccountStorageType},
    assets::{Asset, FungibleAsset},
    notes::{NoteFile, NoteTag, NoteType},
};

mod common;
use common::*;

// mod custom_transactions_tests;
// mod onchain_tests;
mod partial_swap_onchain;

#[tokio::test]
async fn test_added_notes() {
    let mut client = create_test_client();
    wait_for_node(&mut client).await;

    let (_, _, faucet_account_stub) = setup(&mut client, AccountStorageType::OffChain).await;

    // Mint some asset for an account not tracked by the client. It should not be stored as an
    // input note afterwards since it is not being tracked by the client
    let fungible_asset = FungibleAsset::new(faucet_account_stub.id(), MINT_AMOUNT).unwrap();
    let tx_template = TransactionTemplate::MintFungibleAsset(
        fungible_asset,
        AccountId::try_from(ACCOUNT_ID_REGULAR).unwrap(),
        NoteType::OffChain,
    );
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    println!("Running Mint tx...");
    execute_tx_and_sync(&mut client, tx_request).await;

    // Check that no new notes were added
    println!("Fetching Committed Notes...");
    let notes = client.get_input_notes(NoteFilter::Committed).unwrap();
    assert!(notes.is_empty())
}

#[tokio::test]
async fn test_p2id_transfer() {
    let mut client = create_test_client();
    wait_for_node(&mut client).await;

    let (first_regular_account, second_regular_account, faucet_account_stub) =
        setup(&mut client, AccountStorageType::OffChain).await;

    let from_account_id = first_regular_account.id();
    let to_account_id = second_regular_account.id();
    let faucet_account_id = faucet_account_stub.id();

    // First Mint necesary token
    let note = mint_note(
        &mut client,
        from_account_id,
        faucet_account_id,
        NoteType::OffChain,
    )
    .await;
    consume_notes(&mut client, from_account_id, &[note]).await;
    assert_account_has_single_asset(&client, from_account_id, faucet_account_id, MINT_AMOUNT).await;

    // Do a transfer from first account to second account
    let asset = FungibleAsset::new(faucet_account_id, TRANSFER_AMOUNT).unwrap();
    let tx_template = TransactionTemplate::PayToId(
        PaymentTransactionData::new(Asset::Fungible(asset), from_account_id, to_account_id),
        NoteType::OffChain,
    );
    println!("Running P2ID tx...");
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(&mut client, tx_request).await;

    // Check that note is committed for the second account to consume
    println!("Fetching Committed Notes...");
    let notes = client.get_input_notes(NoteFilter::Committed).unwrap();
    assert!(!notes.is_empty());

    // Consume P2ID note
    let tx_template = TransactionTemplate::ConsumeNotes(to_account_id, vec![notes[0].id()]);
    println!("Consuming Note...");
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(&mut client, tx_request).await;

    // Ensure we have nothing else to consume
    let current_notes = client.get_input_notes(NoteFilter::Committed).unwrap();
    assert!(current_notes.is_empty());

    let (regular_account, seed) = client.get_account(from_account_id).unwrap();

    // The seed should not be retrieved due to the account not being new
    assert!(!regular_account.is_new() && seed.is_none());
    assert_eq!(regular_account.vault().assets().count(), 1);
    let asset = regular_account.vault().assets().next().unwrap();

    // Validate the transfered amounts
    if let Asset::Fungible(fungible_asset) = asset {
        assert_eq!(fungible_asset.amount(), MINT_AMOUNT - TRANSFER_AMOUNT);
    } else {
        panic!("Error: Account should have a fungible asset");
    }

    let (regular_account, _seed) = client.get_account(to_account_id).unwrap();
    assert_eq!(regular_account.vault().assets().count(), 1);
    let asset = regular_account.vault().assets().next().unwrap();

    if let Asset::Fungible(fungible_asset) = asset {
        assert_eq!(fungible_asset.amount(), TRANSFER_AMOUNT);
    } else {
        panic!("Error: Account should have a fungible asset");
    }

    assert_note_cannot_be_consumed_twice(&mut client, to_account_id, notes[0].id()).await;
}
