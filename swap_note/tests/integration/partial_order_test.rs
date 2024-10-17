use crate::common::*;
use miden_client::transactions::OutputNote;
use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{account_id::testing::ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN, AccountId},
    assets::{Asset, FungibleAsset},
    notes::NoteType,
    testing::account_code::{self, DEFAULT_AUTH_SCRIPT},
    transaction::TransactionScript,
    Felt,
};
use miden_tx::testing::mock_chain::{Auth, MockChain};

#[test]
fn prove_partial_public_swap_script() {
    // Set up mock chain and assets
    let mut chain = MockChain::new();
    let faucet = chain.add_existing_faucet(Auth::NoAuth, "POL", 100000u64);
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();

    let offered_asset_amount: u64 = 100;
    let offered_asset = faucet.mint(offered_asset_amount); // Offered asset to swap

    let requested_asset_amount: u64 = 100;
    let requested_asset: Asset = FungibleAsset::new(faucet_id_2, requested_asset_amount)
        .unwrap()
        .into(); // Requested asset for swap

    let requested_asset_available: u64 = 20;
    let requested_available: Asset = FungibleAsset::new(faucet_id_2, requested_asset_available)
        .unwrap()
        .into(); // Amount to swap

    // offered_asset_amount / requested_asset_amount * requested_asset_available = amount_out
    // new_offered_asset_amount = offered_asset_amount - amount_out
    // new_requested_asset_amount = requested_asset_amount - requested_asset_available

    // Create accounts for sender and target
    let sender_account = chain.add_new_wallet(Auth::BasicAuth, vec![offered_asset]);
    let target_account = chain.add_existing_wallet(Auth::BasicAuth, vec![requested_available]);

    // Set up the swap transaction
    let serial_num = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];
    let fill_number = 0;

    // Create the partial swap note
    let swap_note = create_partial_swap_note(
        sender_account.id(),
        sender_account.id(),
        offered_asset,
        requested_asset,
        serial_num,
        fill_number,
    )
    .unwrap();

    chain.add_note(swap_note.clone());
    chain.seal_block(None);

    // Set up the expected remaining assets after partial fill
    let offered_remaining = faucet.mint(80);
    let requested_remaining = FungibleAsset::new(faucet_id_2, 80).unwrap().into();

    let output_swap_note = create_partial_swap_note(
        sender_account.id(),
        target_account.id(),
        offered_remaining,
        requested_remaining,
        serial_num,
        fill_number + 1,
    )
    .unwrap();

    // Expected output note
    let p2id_serial_num = compute_p2id_serial_num(serial_num, fill_number + 1);
    let expected_p2id_note = create_p2id_note(
        target_account.id(),
        sender_account.id(),
        vec![requested_available],
        NoteType::Public,
        Felt::new(0),
        p2id_serial_num,
    )
    .unwrap();

    let expected_swap_note = OutputNote::Full(output_swap_note);
    let expected_p2id_note = OutputNote::Full(expected_p2id_note);

    // Construct and execute the transaction
    let tx_script =
        TransactionScript::compile(DEFAULT_AUTH_SCRIPT, vec![], TransactionKernel::assembler())
            .unwrap();

    let executed_transaction = chain
        .build_tx_context(target_account.id())
        .tx_script(tx_script)
        .expected_notes(vec![expected_p2id_note.clone(), expected_swap_note.clone()])
        .build()
        .execute()
        .unwrap();

    // Assert that the P2ID recipient digest matches
    assert_eq!(
        executed_transaction
            .output_notes()
            .get_note(0)
            .recipient_digest(),
        expected_p2id_note.recipient_digest(),
        "P2ID recipient digests do not match"
    );

    // Assert that the swap recipient digest matches
    assert_eq!(
        executed_transaction
            .output_notes()
            .get_note(1)
            .recipient_digest(),
        expected_swap_note.recipient_digest(),
        "SWAP recipient digests do not match"
    );

    // Assert that the P2ID assets match
    assert_eq!(
        executed_transaction.output_notes().get_note(0).assets(),
        expected_p2id_note.assets(),
        "P2ID assets do not match"
    );

    // Assert that the swap assets match
    assert_eq!(
        executed_transaction.output_notes().get_note(1).assets(),
        expected_swap_note.assets(),
        "SWAP assets do not match"
    );

    let mut asset_swap_note = executed_transaction
        .output_notes()
        .get_note(1)
        .assets()
        .unwrap()
        .iter();

    // Get the first (and only) asset
    if let Some(first_asset) = asset_swap_note.next() {
        // Now you have the first asset in `first_asset`
        println!("New SWAPp asset balance: {:?}", first_asset);
    } else {
        panic!("should have asset");
    }

    let mut asset_p2id_note = executed_transaction
        .output_notes()
        .get_note(0)
        .assets()
        .unwrap()
        .iter();

    // Get the first (and only) asset
    if let Some(first_asset) = asset_p2id_note.next() {
        // Now you have the first asset in `first_asset`
        println!("P2ID for creator balance: {:?}", first_asset);
    } else {
        panic!("should have asset");
    }

    let account_delta = executed_transaction.account_delta().vault().fungible();
    println!("account delta: {:?}", account_delta);
}

#[test]
fn prove_partial_public_swap_script_with_precision() {
    execute_partial_public_swap_test(355_000_000, 113_000_000, 100_000);
}

fn execute_partial_public_swap_test(
    offered_asset_amount: u64,
    requested_asset_amount: u64,
    requested_asset_available: u64,
) {
    // SWAPp formula:
    // amount_out = (offered_asset_amount * requested_asset_available) / requested_asset_amount
    // new_offered_asset_amount = offered_asset_amount - amount_out
    // new_requested_asset_amount = requested_asset_amount - requested_asset_available

    // Compute amount_out and new amounts
    let amount_out = (offered_asset_amount * requested_asset_available) / requested_asset_amount;
    let new_offered_asset_amount = offered_asset_amount - amount_out;
    let new_requested_asset_amount = requested_asset_amount - requested_asset_available;

    // Set up mock chain and assets
    let mut chain = MockChain::new();
    let faucet = chain.add_existing_faucet(Auth::NoAuth, "POL", 100000u64);
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();

    let offered_asset = faucet.mint(offered_asset_amount);

    let requested_asset: Asset = FungibleAsset::new(faucet_id_2, requested_asset_amount)
        .unwrap()
        .into();

    let requested_available: Asset = FungibleAsset::new(faucet_id_2, requested_asset_available)
        .unwrap()
        .into();

    // Create accounts for sender and target
    let sender_account = chain.add_new_wallet(Auth::BasicAuth, vec![offered_asset]);
    let target_account = chain.add_existing_wallet(Auth::BasicAuth, vec![requested_available]);

    // Set up the swap transaction
    let serial_num = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];
    let fill_number = 0;

    // Create the partial swap note
    let swap_note = create_partial_swap_note(
        sender_account.id(),
        sender_account.id(),
        offered_asset,
        requested_asset,
        serial_num,
        fill_number,
    )
    .unwrap();

    chain.add_note(swap_note.clone());
    chain.seal_block(None);

    // Set up the expected remaining assets after partial fill
    let offered_remaining = faucet.mint(new_offered_asset_amount);
    let requested_remaining = FungibleAsset::new(faucet_id_2, new_requested_asset_amount)
        .unwrap()
        .into();

    let output_swap_note = create_partial_swap_note(
        sender_account.id(),
        target_account.id(),
        offered_remaining,
        requested_remaining,
        serial_num,
        fill_number + 1,
    )
    .unwrap();

    // Expected output note
    let p2id_serial_num = compute_p2id_serial_num(serial_num, fill_number + 1);
    let expected_p2id_note = create_p2id_note(
        target_account.id(),
        sender_account.id(),
        vec![requested_available],
        NoteType::Public,
        Felt::new(0),
        p2id_serial_num,
    )
    .unwrap();

    let expected_swap_note = OutputNote::Full(output_swap_note);
    let expected_p2id_note = OutputNote::Full(expected_p2id_note);

    // Construct and execute the transaction
    let tx_script =
        TransactionScript::compile(DEFAULT_AUTH_SCRIPT, vec![], TransactionKernel::assembler())
            .unwrap();

    let executed_transaction = chain
        .build_tx_context(target_account.id())
        .tx_script(tx_script)
        .expected_notes(vec![expected_p2id_note.clone(), expected_swap_note.clone()])
        .build()
        .execute()
        .unwrap();

    chain.add_executed_transaction(executed_transaction.clone());
    chain.seal_block(None);

    // Assert that the P2ID recipient digest matches
    assert_eq!(
        executed_transaction
            .output_notes()
            .get_note(0)
            .recipient_digest(),
        expected_p2id_note.recipient_digest(),
        "P2ID recipient digests do not match"
    );

    // Assert that the swap recipient digest matches
    assert_eq!(
        executed_transaction
            .output_notes()
            .get_note(1)
            .recipient_digest(),
        expected_swap_note.recipient_digest(),
        "SWAP recipient digests do not match"
    );

    // Assert that the P2ID assets match
    assert_eq!(
        executed_transaction.output_notes().get_note(0).assets(),
        expected_p2id_note.assets(),
        "P2ID assets do not match"
    );

    // Assert that the swap assets match
    assert_eq!(
        executed_transaction.output_notes().get_note(1).assets(),
        expected_swap_note.assets(),
        "SWAP assets do not match"
    );

    let mut asset_swap_note = executed_transaction
        .output_notes()
        .get_note(1)
        .assets()
        .unwrap()
        .iter();

    if let Some(first_asset) = asset_swap_note.next() {
        println!("New SWAPp asset balance: {:?}", first_asset);
    } else {
        panic!("should have asset");
    }

    let mut asset_p2id_note = executed_transaction
        .output_notes()
        .get_note(0)
        .assets()
        .unwrap()
        .iter();

    if let Some(first_asset) = asset_p2id_note.next() {
        println!("P2ID for creator balance: {:?}", first_asset);
    } else {
        panic!("should have asset");
    }

    let account_delta = executed_transaction.account_delta().vault().fungible();
    println!("account delta: {:?}", account_delta);
}
