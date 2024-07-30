use miden_objects::{
    accounts::AccountId,
    assembly::ProgramAst,
    assets::{Asset, FungibleAsset},
    notes::NoteType,
    transaction::TransactionArgs,
    Felt,
};
use miden_processor::AdviceMap;
use miden_tx::{testing::TransactionContextBuilder, TransactionExecutor};

use crate::common::*;

// @dev Test that the is_consumer_is_creator procedure succeeds when the consumer is not the creator
#[test]
fn test_is_consumer_creator_reclaim_success() {
    // ASSETS
    // Offered Asset
    let faucet_id_1 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let amount_token_a: u64 = format_value_with_decimals(607, 6);
    let offered_token_a: Asset = FungibleAsset::new(faucet_id_1, amount_token_a)
        .unwrap()
        .into();

    // Requested Asset
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let requested_amount_token_b = format_value_with_decimals(987, 6);
    let requested_token_b: Asset = FungibleAsset::new(faucet_id_2, requested_amount_token_b)
        .unwrap()
        .into();

    // ACCOUNT IDs
    // SWAPp note creator
    let swapp_creator_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // SWAPp note consumer
    let swapp_consumer_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // SWAPp note consumer wallet balance
    let token_b_amount_in = format_value_with_decimals(387, 6);
    let swap_consumer_token_b = FungibleAsset::new(faucet_id_2, token_b_amount_in)
        .unwrap()
        .into();
    let (target_pub_key, target_falcon_auth) = get_new_pk_and_authenticator();

    // SWAPp note consumer wallet
    let swap_consumer_wallet = get_custom_account_code(
        swapp_consumer_account_id,
        target_pub_key,
        Some(swap_consumer_token_b),
    );

    let fill_number = 0;

    // SWAPp note
    let (swap_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id.clone(),
        swapp_creator_account_id.clone(),
        offered_token_a,
        requested_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        fill_number,
    )
    .unwrap();

    // CONSTRUCT AND EXECUTE TX (Success)
    // --------------------------------------------------------------------------------------------
    let tx_context = TransactionContextBuilder::new(swap_consumer_wallet.clone())
        .input_notes(vec![swap_note.clone()])
        .build();

    let mut executor =
        TransactionExecutor::new(tx_context.clone(), Some(target_falcon_auth.clone()))
            .with_debug_mode(true);
    executor.load_account(swapp_consumer_account_id).unwrap();

    let block_ref = tx_context.tx_inputs().block_header().block_num();
    let note_ids = tx_context
        .tx_inputs()
        .input_notes()
        .iter()
        .map(|note| note.id())
        .collect::<Vec<_>>();

    let tx_script_code = include_str!("../../src/tx_scripts/tx_script.masm");
    let tx_script_ast = ProgramAst::parse(tx_script_code).unwrap();

    let tx_script_target = executor
        .compile_tx_script(tx_script_ast.clone(), vec![], vec![])
        .unwrap();

    let tx_args_target = TransactionArgs::new(Some(tx_script_target), None, AdviceMap::default());

    // Execute the transaction and get the witness
    let executed_transaction = executor.execute_transaction(
        swapp_consumer_account_id,
        block_ref,
        &note_ids,
        tx_args_target.clone(),
    );
    assert!(executed_transaction.is_ok());
}

// @dev Test that the is_consumer_is_creator procedure fails when the consumer is not the creator
#[test]
fn test_is_consumer_creator_unauthorized() {
    // ASSETS
    // Offered Asset
    let faucet_id_1 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let amount_token_a: u64 = format_value_with_decimals(607, 6);
    let offered_token_a: Asset = FungibleAsset::new(faucet_id_1, amount_token_a)
        .unwrap()
        .into();

    // Requested Asset
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let requested_amount_token_b = format_value_with_decimals(987, 6);
    let requested_token_b: Asset = FungibleAsset::new(faucet_id_2, requested_amount_token_b)
        .unwrap()
        .into();

    // ACCOUNT IDs
    // SWAPp note creator
    let swapp_creator_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // SWAPp note consumer
    let swapp_consumer_account_id = AccountId::try_from(ACCOUNT_ID_SENDER_1).unwrap();

    // SWAPp note consumer wallet balance
    let token_b_amount_in = format_value_with_decimals(387, 6);
    let swap_consumer_token_b = FungibleAsset::new(faucet_id_2, token_b_amount_in)
        .unwrap()
        .into();
    let (target_pub_key, target_falcon_auth) = get_new_pk_and_authenticator();

    // SWAPp note consumer wallet
    let swap_consumer_wallet = get_custom_account_code(
        swapp_consumer_account_id,
        target_pub_key,
        Some(swap_consumer_token_b),
    );

    let fill_number = 0;

    // SWAPp note
    let (swap_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id.clone(),
        swapp_creator_account_id.clone(),
        offered_token_a,
        requested_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        fill_number,
    )
    .unwrap();

    // CONSTRUCT AND EXECUTE TX (Success)
    // --------------------------------------------------------------------------------------------
    let tx_context = TransactionContextBuilder::new(swap_consumer_wallet.clone())
        .input_notes(vec![swap_note.clone()])
        .build();

    let mut executor =
        TransactionExecutor::new(tx_context.clone(), Some(target_falcon_auth.clone()))
            .with_debug_mode(true);
    executor.load_account(swapp_consumer_account_id).unwrap();

    let block_ref = tx_context.tx_inputs().block_header().block_num();
    let note_ids = tx_context
        .tx_inputs()
        .input_notes()
        .iter()
        .map(|note| note.id())
        .collect::<Vec<_>>();

    let tx_script_code = include_str!("../../src/tx_scripts/tx_script.masm");
    let tx_script_ast = ProgramAst::parse(tx_script_code).unwrap();

    let tx_script_target = executor
        .compile_tx_script(tx_script_ast.clone(), vec![], vec![])
        .unwrap();

    let tx_args_target = TransactionArgs::new(Some(tx_script_target), None, AdviceMap::default());

    // Execute the transaction and get the witness
    let executed_transaction = executor.execute_transaction(
        swapp_consumer_account_id,
        block_ref,
        &note_ids,
        tx_args_target.clone(),
    );

    assert!(executed_transaction.is_err());
}
