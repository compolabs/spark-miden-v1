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
fn prove_partial_public_swap_script() {
    // Create assets
    let mut chain = MockChain::new();
    let faucet = chain.add_existing_faucet(Auth::NoAuth, "POL", 100000u64);
    let offered_asset = faucet.mint(100); // offered asset in note

    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let requested_asset: Asset = FungibleAsset::new(faucet_id_2, 100).unwrap().into(); // requested asset in note
    let requested_available: Asset = FungibleAsset::new(faucet_id_2, 50).unwrap().into(); // amount to swap in account

    // Create sender and target account
    let sender_account = chain.add_new_wallet(Auth::BasicAuth, vec![offered_asset]);
    let target_account = chain.add_existing_wallet(Auth::BasicAuth, vec![requested_available]);

    let serial_num = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];
    let fill_number = 0;

    // Create the note containing the SWAP script
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

    // expected note
    let offered_remaining = faucet.mint(50);
    let requested_remaning = FungibleAsset::new(faucet_id_2, 50).unwrap().into();

    let (output_swap_note, _note_script_hash) = create_partial_swap_note_test(
        sender_account.id(),
        target_account.id(),
        offered_remaining,
        requested_remaning,
        NoteType::Public,
        serial_num,
        fill_number + 1,
    )
    .unwrap();

    let p2id_serial_num = compute_p2id_serial_num(serial_num, fill_number + 1);

    let expected_output_p2id = create_p2id_note(
        target_account.id(),
        sender_account.id(),
        vec![requested_available],
        NoteType::Public,
        Felt::new(0),
        p2id_serial_num,
    )
    .unwrap();

    let expected_swapp_note: OutputNote =
        miden_objects::transaction::OutputNote::Full(output_swap_note);

    let expected_p2id_note: OutputNote =
        miden_objects::transaction::OutputNote::Full(expected_output_p2id);

    // CONSTRUCT AND EXECUTE TX (Success)
    // --------------------------------------------------------------------------------------------

    let tx_script =
        TransactionScript::compile(DEFAULT_AUTH_SCRIPT, vec![], TransactionKernel::assembler())
            .unwrap();

    // this results in "Public note missing the details in the advice provider"

    let executed_transaction = chain
        .build_tx_context(target_account.id())
        .tx_script(tx_script)
        .expected_notes(vec![
            expected_p2id_note.clone(),
            expected_swapp_note.clone(),
        ])
        .build()
        .execute()
        .unwrap();

    // Prove, serialize/deserialize and verify the transaction
    // assert!(prove_and_verify_transaction(executed_transaction.clone()).is_ok());

    // Assert that the p2id recipient digest is the same
    assert_eq!(
        executed_transaction
            .output_notes()
            .get_note(0)
            .recipient_digest(),
        expected_p2id_note.recipient_digest(),
        "P2ID recipient digests do not match"
    );

    // Assert that the swapp recipient digest is the same
    assert_eq!(
        executed_transaction
            .output_notes()
            .get_note(1)
            .recipient_digest(),
        expected_swapp_note.recipient_digest(),
        "SWAPp recipient digests do not match"
    );

    assert_eq!(
        executed_transaction.output_notes().get_note(0).assets(),
        expected_p2id_note.assets(),
        "P2ID assets do not match"
    );

    assert_eq!(
        executed_transaction.output_notes().get_note(1).assets(),
        expected_swapp_note.assets(),
        "SWAPp assets do not match"
    );
}
