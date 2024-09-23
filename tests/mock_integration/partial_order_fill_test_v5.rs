use crate::common::*;
use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{account_id::testing::ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN, Account, AccountId},
    assets::{Asset, AssetVault, FungibleAsset},
    notes::{
        NoteAssets, NoteExecutionHint, NoteExecutionMode, NoteHeader, NoteId, NoteMetadata,
        NoteTag, NoteType,
    },
    testing::account_code::DEFAULT_AUTH_SCRIPT,
    transaction::TransactionScript,
    transaction::{ProvenTransaction, TransactionArgs},
    Felt, ZERO,
};
use miden_tx::testing::mock_chain::{Auth, MockChain};
use miden_tx::{testing::TransactionContextBuilder, TransactionExecutor};

#[test]
fn prove_partial_swap_script() {
    // Create assets
    let mut chain = MockChain::new();
    let faucet = chain.add_existing_faucet(Auth::NoAuth, "POL", 100000u64);
    let offered_asset = faucet.mint(100);

    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let requested_asset: Asset = FungibleAsset::new(faucet_id_2, 100).unwrap().into();

    // Create sender and target account
    let sender_account = chain.add_new_wallet(Auth::BasicAuth, vec![offered_asset]);
    let target_account = chain.add_existing_wallet(Auth::BasicAuth, vec![requested_asset]);

    let serial_num = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];
    let fill_number = 0;

    println!("HINT: {:?}", Felt::from(NoteExecutionHint::always()));

    // Create the note containing the SWAP script
    let (swap_note, payback_note, _note_script_hash) = create_partial_swap_note(
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

    // CONSTRUCT AND EXECUTE TX (Success)
    // --------------------------------------------------------------------------------------------

    let tx_script =
        TransactionScript::compile(DEFAULT_AUTH_SCRIPT, vec![], TransactionKernel::assembler())
            .unwrap();

    // Adding tx args
    let tx_context =
        TransactionContextBuilder::with_standard_account(target_account.id().into()).build();

    let executor: TransactionExecutor<_, ()> = TransactionExecutor::new(tx_context.clone(), None);
    let account_id = tx_context.tx_inputs().account().id();

    let tx_args = TransactionArgs::new(
        Some(tx_script.clone()),
        None,
        tx_context.tx_args().advice_inputs().clone().map,
    );

    let block_ref = tx_context.tx_inputs().block_header().block_num();
    let note_ids = tx_context
        .tx_inputs()
        .input_notes()
        .iter()
        .map(|note| note.id())
        .collect::<Vec<_>>();

    // execute the transaction and get the witness
    let executed_transaction = executor
        .execute_transaction(account_id, block_ref, &note_ids, tx_args)
        .unwrap();

    // this results in "Public note missing the details in the advice provider"
    /*
    let executed_transaction = chain
           .build_tx_context(target_account.id())
           .tx_script(tx_script)
           .build()
           .execute()
           .unwrap();
    */

    // Prove, serialize/deserialize and verify the transaction
    assert!(prove_and_verify_transaction(executed_transaction.clone()).is_ok());
}
