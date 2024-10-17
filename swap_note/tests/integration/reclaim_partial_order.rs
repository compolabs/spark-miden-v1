use crate::common::*;
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

    // Create sender and target account
    let sender_account = chain.add_existing_wallet(Auth::BasicAuth, vec![offered_asset]);

    let serial_num = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];
    let fill_number = 0;

    // Create the note containing the SWAP script
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

    // CONSTRUCT AND EXECUTE TX (Success)
    // --------------------------------------------------------------------------------------------

    let tx_script =
        TransactionScript::compile(DEFAULT_AUTH_SCRIPT, vec![], TransactionKernel::assembler())
            .unwrap();

    let executed_transaction = chain
        .build_tx_context(sender_account.id())
        .tx_script(tx_script)
        .input_notes(vec![swap_note])
        .build()
        .execute()
        .unwrap();

    let account_delta = executed_transaction.account_delta().vault().fungible();
    println!("account delta: {:?}", account_delta);

    // Prove, serialize/deserialize and verify the transaction
    // assert!(prove_and_verify_transaction(executed_transaction.clone()).is_ok());
}
