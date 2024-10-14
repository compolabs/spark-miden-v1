use crate::common::*;
use miden_client::transactions::OutputNote;
use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{account_id::testing::ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN, AccountId},
    assets::{Asset, FungibleAsset},
    notes::NoteType,
    testing::account_code::DEFAULT_AUTH_SCRIPT,
    transaction::TransactionScript,
    Felt,
};
use miden_tx::testing::mock_chain::{Auth, MockChain};

#[test]
fn prove_complete_order_fill() {
    // Set up mock chain and assets
    let mut chain = MockChain::new();
    let faucet = chain.add_existing_faucet(Auth::NoAuth, "POL", 100000u64);
    let offered_asset = faucet.mint(100); // Offered asset to swap

    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let requested_asset: Asset = FungibleAsset::new(faucet_id_2, 100).unwrap().into(); // Requested asset for swap
    let requested_available: Asset = FungibleAsset::new(faucet_id_2, 100).unwrap().into(); // Amount to swap

    // Create accounts for sender and target
    let sender_account = chain.add_new_wallet(Auth::BasicAuth, vec![offered_asset]);
    let target_account = chain.add_existing_wallet(Auth::BasicAuth, vec![requested_available]);

    // Set up the swap transaction
    let serial_num = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];
    let fill_number = 0;

    // Create the partial swap note
    let (swap_note, _note_script_hash) = create_partial_swap_note_test(
        sender_account.id(),
        sender_account.id(),
        offered_asset,
        requested_asset,
        NoteType::Public,
        serial_num,
        fill_number,
    )
    .unwrap();

    chain.add_note(swap_note.clone());
    chain.seal_block(None);

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

    let expected_p2id_note = OutputNote::Full(expected_p2id_note);

    // Construct and execute the transaction
    let tx_script =
        TransactionScript::compile(DEFAULT_AUTH_SCRIPT, vec![], TransactionKernel::assembler())
            .unwrap();

    let executed_transaction = chain
        .build_tx_context(target_account.id())
        .tx_script(tx_script)
        .expected_notes(vec![expected_p2id_note.clone()])
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

    // Assert that the P2ID assets match
    assert_eq!(
        executed_transaction.output_notes().get_note(0).assets(),
        expected_p2id_note.assets(),
        "P2ID assets do not match"
    );
}
