use std::os::unix::fs::lchown;

use crate::common::*;
use miden_client::transactions::OutputNote;
use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{
        account_id::testing::{
            ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN, ACCOUNT_ID_OFF_CHAIN_SENDER,
            ACCOUNT_ID_REGULAR_ACCOUNT_UPDATABLE_CODE_OFF_CHAIN,
            ACCOUNT_ID_REGULAR_ACCOUNT_UPDATABLE_CODE_ON_CHAIN_2,
        },
        Account, AccountId,
    },
    assets::{Asset, AssetVault, FungibleAsset},
    notes::{
        NoteAssets, NoteExecutionHint, NoteExecutionMode, NoteHeader, NoteId, NoteMetadata,
        NoteTag, NoteType,
    },
    testing::{account::AccountBuilder, account_code::DEFAULT_AUTH_SCRIPT},
    transaction::{ProvenTransaction, TransactionArgs, TransactionScript},
    Felt, ZERO,
};
use miden_tx::testing::mock_chain::{Auth, MockChain};
use miden_tx::{testing::TransactionContextBuilder, TransactionExecutor};
use rand_chacha::ChaCha20Rng;

#[test]
fn prove_partial_swap_script() {
    // Create assets
    let mut chain = MockChain::new();
    let faucet = chain.add_existing_faucet(Auth::NoAuth, "POL", 100000u64);
    let offered_asset = faucet.mint(100);

    println!("\n");
    println!(
        "tokenA (offered asset) faucet id: {:?}",
        faucet.account().id()
    );
    println!("\n");

    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let requested_asset: Asset = FungibleAsset::new(faucet_id_2, 100).unwrap().into();
    let requested_available: Asset = FungibleAsset::new(faucet_id_2, 100).unwrap().into();

    println!("\n");
    println!(
        "tokenB (requested asset) faucet id: {:?}",
        ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN
    );
    println!("\n");

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
    let requested_remaning = FungibleAsset::new(faucet_id_2, 100).unwrap().into();

    let (output_swap_note, _note_script_hash) = create_partial_swap_note_test(
        sender_account.id(),
        target_account.id(),
        offered_remaining,
        requested_remaning,
        NoteType::Public,
        serial_num,
        fill_number, // fill_number + 1,
    )
    .unwrap();

    let p2id_serial_num: [Felt; 4] = [Felt::new(0), Felt::new(0), Felt::new(0), Felt::new(0)];
    let expected_output_p2id = create_p2id_note(
        target_account.id(),
        sender_account.id(),
        vec![requested_available],
        NoteType::Public,
        Felt::new(0),
        p2id_serial_num,
    )
    .unwrap();

    println!("\n");
    println!(
        "P2ID script hash: {:?}",
        expected_output_p2id.script().hash()
    );
    println!("\n");

    println!(
        "P2ID recipient: {:?}",
        expected_output_p2id.recipient().digest()
    );
    println!("\n");

    println!("Sender AccountId: {:?}", sender_account.id());
    println!("\n");

    let execution_hint_1 = Felt::from(NoteExecutionHint::always());
    println!("execution hint: {:?}", execution_hint_1);

    let expected_swapp_note: OutputNote =
        miden_objects::transaction::OutputNote::Full(output_swap_note);

    let expected_p2id_note: OutputNote =
        miden_objects::transaction::OutputNote::Full(expected_output_p2id);

    // CONSTRUCT AND EXECUTE TX (Success)
    // --------------------------------------------------------------------------------------------

    let tx_script =
        TransactionScript::compile(DEFAULT_AUTH_SCRIPT, vec![], TransactionKernel::assembler())
            .unwrap();

    let executed_transaction = chain
        .build_tx_context(target_account.id())
        .tx_script(tx_script)
        .expected_notes(vec![expected_p2id_note, expected_swapp_note])
        .build()
        .execute()
        .unwrap();

    // Prove, serialize/deserialize and verify the transaction
    assert!(prove_and_verify_transaction(executed_transaction.clone()).is_ok());

    println!("output {:?}", executed_transaction.output_notes());

    // assert_eq!(executed_transaction.output_notes().get_note(0), expected_p2id_note);
}
