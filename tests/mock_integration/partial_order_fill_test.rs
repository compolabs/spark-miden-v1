// use assert_cmd::assert;
use miden_objects::{
    accounts::AccountId,
    assembly::ProgramAst,
    assets::{Asset, FungibleAsset},
    notes::{NoteAssets, NoteHeader, NoteType},
    transaction::TransactionArgs,
    Felt,
};
use miden_processor::AdviceMap;
use miden_tx::{testing::TransactionContextBuilder, TransactionExecutor};
use std::collections::BTreeMap;

use crate::common::*;

// @dev Test that a SWAPp note can be filled with a partial amount of the requested asset
#[test]
fn test_partial_swap_fill() {
    // ASSETS
    // Offered Asset
    let faucet_id_1 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let amount_token_a: u64 = format_value_with_decimals(100, 6);
    let offered_token_a: Asset = FungibleAsset::new(faucet_id_1, amount_token_a)
        .unwrap()
        .into();

    // Requested Asset
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let requested_amount_token_b = format_value_with_decimals(100, 6);
    let requested_token_b: Asset = FungibleAsset::new(faucet_id_2, requested_amount_token_b)
        .unwrap()
        .into();

    // ACCOUNT IDs
    // SWAPp note creator
    let swapp_creator_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // SWAPp note consumer
    let swapp_consumer_account_id = AccountId::try_from(ACCOUNT_ID_SENDER_1).unwrap();

    // SWAPp note consumer wallet balance
    let token_b_amount_in = format_value_with_decimals(80, 6);
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
    let executed_transaction = executor
        .execute_transaction(
            swapp_consumer_account_id,
            block_ref,
            &note_ids,
            tx_args_target.clone(),
        )
        .unwrap();

    // P2ID & SWAPp note outputted by the transaction
    let p2id_ouput_note = executed_transaction.output_notes().get_note(0);
    let swapp_output_note = executed_transaction.output_notes().get_note(1);

    // Calculate the amount of tokens A that are given to the consumer account
    let offered_token_a_out_amount =
        calculate_tokens_a_for_b(amount_token_a, requested_amount_token_b, token_b_amount_in);

    // Calculate the remaining tokens A and B
    let offered_token_a_amount_remaining = amount_token_a - offered_token_a_out_amount;
    let remaining_token_a: Asset =
        FungibleAsset::new(faucet_id_1, offered_token_a_amount_remaining)
            .unwrap()
            .into();

    let requested_token_b_amount_remaining = requested_amount_token_b - token_b_amount_in;
    let remaining_token_b: Asset =
        FungibleAsset::new(faucet_id_2, requested_token_b_amount_remaining)
            .unwrap()
            .into();

    // Note expected to be outputted by the transaction
    let (expected_swapp_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id,
        swapp_consumer_account_id,
        remaining_token_a,
        remaining_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        fill_number + 1,
    )
    .unwrap();

    assert_eq!(executed_transaction.output_notes().num_notes(), 2);

    // Check that the output note is the same as the expected note
    assert_eq!(
        NoteHeader::from(swapp_output_note).metadata(),
        NoteHeader::from(expected_swapp_note.clone()).metadata()
    );

    assert_eq!(
        NoteHeader::from(swapp_output_note),
        NoteHeader::from(expected_swapp_note.clone())
    );

    // @dev comment out to speed up test
    // assert!(prove_and_verify_transaction(executed_transaction.clone()).is_ok());

    // Checking ouputted SWAPp and P2ID notes contain the correct amount of liquidity
    let p2id_note_balance: &NoteAssets = p2id_ouput_note.assets().unwrap();
    let swapp_note_balance: &NoteAssets = swapp_output_note.assets().unwrap();

    let mut asset_iter = swapp_note_balance.iter();
    let swap_note_asset = asset_iter.next().expect("Expected at least one asset");

    let token_a_remaining_in_swap = match swap_note_asset {
        Asset::Fungible(fa) => fa,
        _ => panic!("Expected a fungible asset, but found a non-fungible asset."),
    };

    let asset_id_in_swapp_note = token_a_remaining_in_swap.faucet_id();
    let token_a_amount_in_swapp_note = token_a_remaining_in_swap.amount();

    let mut asset_iter = p2id_note_balance.iter();
    let p2id_note_asset = asset_iter.next().expect("Expected at least one asset");

    let token_b_output_in_p2id = match p2id_note_asset {
        Asset::Fungible(fa) => fa,
        _ => panic!("Expected a fungible asset, but found a non-fungible asset."),
    };

    let asset_id_in_p2id_note = token_b_output_in_p2id.faucet_id();
    let token_b_amount_in_p2id_note = token_b_output_in_p2id.amount();

    assert_eq!(p2id_note_balance.num_assets(), 1);
    assert_eq!(swapp_note_balance.num_assets(), 1);

    assert_eq!(asset_id_in_swapp_note, faucet_id_1);
    assert_eq!(
        token_a_amount_in_swapp_note,
        offered_token_a_amount_remaining
    );

    assert_eq!(asset_id_in_p2id_note, faucet_id_2);
    assert_eq!(token_b_amount_in_p2id_note, token_b_amount_in);
}

// @dev Test that a SWAPp note can be filled with a partial amount of the requested asset
#[test]
fn test_partial_swap_fill_graphical() {
    // ASSETS
    // Offered Asset (tokenA)
    let amount_a: u64 = 10;

    let faucet_id_1 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let amount_token_a: u64 = format_value_with_decimals(amount_a, 8);
    let offered_token_a: Asset = FungibleAsset::new(faucet_id_1, amount_token_a)
        .unwrap()
        .into();

    // Requested Asset (tokenB)
    let amount_b: u64 = 20;

    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let requested_amount_token_b = format_value_with_decimals(amount_b, 8);
    let requested_token_b: Asset = FungibleAsset::new(faucet_id_2, requested_amount_token_b)
        .unwrap()
        .into();

    // ACCOUNT IDs
    // SWAPp note creator
    let swapp_creator_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // SWAPp note consumer
    let swapp_consumer_account_id = AccountId::try_from(ACCOUNT_ID_SENDER_1).unwrap();

    // SWAPp note consumer wallet balance
    let amount_b_in: u64 = 5;

    let token_b_amount_in = format_value_with_decimals(amount_b_in, 8);
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

    // SWAPp note
    let (swap_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id.clone(),
        swapp_creator_account_id.clone(),
        offered_token_a,
        requested_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        0,
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

    // TRANSACTION EXECUTION
    // --------------------------------------------------------------------------------------------
    let executed_transaction = executor
        .execute_transaction(
            swapp_consumer_account_id,
            block_ref,
            &note_ids,
            tx_args_target.clone(),
        )
        .unwrap();

    // EXECUTION OUTPUT NOTES
    // P2ID & SWAPp notes outputted by the transaction
    let p2id_ouput_note = executed_transaction.output_notes().get_note(0);
    let swapp_output_note = executed_transaction.output_notes().get_note(1);

    // Calculate the expected amount of tokens A that are given to the consumer account
    let expected_token_a_out_amount =
        calculate_tokens_a_for_b(amount_token_a, requested_amount_token_b, token_b_amount_in);

    // Calculate the remaining tokens A and B
    let expected_token_a_amount_remaining = amount_token_a - expected_token_a_out_amount;
    let remaining_token_a: Asset =
        FungibleAsset::new(faucet_id_1, expected_token_a_amount_remaining)
            .unwrap()
            .into();

    let requested_token_b_amount_remaining = requested_amount_token_b - token_b_amount_in;
    let remaining_token_b: Asset =
        FungibleAsset::new(faucet_id_2, requested_token_b_amount_remaining)
            .unwrap()
            .into();

    // Note expected to be outputted by the transaction
    let (expected_swapp_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id,
        swapp_consumer_account_id,
        remaining_token_a,
        remaining_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        0,
    )
    .unwrap();

    assert_eq!(executed_transaction.output_notes().num_notes(), 2);

    // Check that the output note is the same as the expected note
    assert_eq!(
        NoteHeader::from(swapp_output_note).metadata(),
        NoteHeader::from(expected_swapp_note.clone()).metadata()
    );

    assert_eq!(
        NoteHeader::from(swapp_output_note),
        NoteHeader::from(expected_swapp_note.clone())
    );

    // @dev comment out to speed up test
    // assert!(prove_and_verify_transaction(executed_transaction.clone()).is_ok());

    // Checking ouputted SWAPp and P2ID notes contain the correct amount of liquidity
    let p2id_note_balance: &NoteAssets = p2id_ouput_note.assets().unwrap();
    let swapp_note_balance: &NoteAssets = swapp_output_note.assets().unwrap();

    let mut asset_iter = swapp_note_balance.iter();
    let swap_note_asset = asset_iter.next().expect("Expected at least one asset");

    let token_a_remaining_in_swap = match swap_note_asset {
        Asset::Fungible(fa) => fa,
        _ => panic!("Expected a fungible asset, but found a non-fungible asset."),
    };

    let asset_id_in_swapp_note = token_a_remaining_in_swap.faucet_id();
    let token_a_amount_in_swapp_note = token_a_remaining_in_swap.amount();

    let mut asset_iter = p2id_note_balance.iter();
    let p2id_note_asset = asset_iter.next().expect("Expected at least one asset");

    let token_b_output_in_p2id = match p2id_note_asset {
        Asset::Fungible(fa) => fa,
        _ => panic!("Expected a fungible asset, but found a non-fungible asset."),
    };

    let asset_id_in_p2id_note = token_b_output_in_p2id.faucet_id();
    let token_b_amount_in_p2id_note = token_b_output_in_p2id.amount();

    // ASSERT BALANCES OF OUTPUTTED NOTES
    assert_eq!(p2id_note_balance.num_assets(), 1);
    assert_eq!(swapp_note_balance.num_assets(), 1);
    assert_eq!(asset_id_in_swapp_note, faucet_id_1);
    assert_eq!(asset_id_in_p2id_note, faucet_id_2);

    assert_eq!(
        token_a_amount_in_swapp_note,
        expected_token_a_amount_remaining
    );
    assert_eq!(token_b_amount_in_p2id_note, token_b_amount_in);

    println!("SWAPp NOTE BALANCE A: {}", amount_a);
    println!("SWAPp NOTE REQUESTED B: {}", amount_b);
    println!(
        "TOKEN B AMOUNT IN: {}",
        format_value_to_float(token_b_amount_in, 8)
    );

    println!(
        "P2ID NOTE BALANCE: {}",
        format_value_to_float(token_b_amount_in_p2id_note, 8)
    );
    println!(
        "SWAPp' NOTE BALANCE TOKEN A: {}",
        format_value_to_float(token_a_amount_in_swapp_note, 8)
    );

    println!("/* ______________________________________ */");
    println!("\n");

    // Circle representation for SWAPp NOTE
    println!("              ________________");
    println!("            /                  \\");
    println!("           | SWAPp NOTE (Alice)|");
    println!("           |  {} tokens A      |", amount_a);
    println!("           |        for        |");
    println!("           |  {} tokens B      |", amount_b);
    println!("            \\__________________/");
    println!("                     |");
    println!("                     |");
    println!("                     V");

    // Rectangle for Consuming Account
    println!("         ___________________________");
    println!("         |  Consuming Account (Bob) |");
    println!(
        "         |       {} tokens B         |",
        amount_b_in.clone()
    );
    println!("         |__________________________|");
    println!("          |                       |   ");
    println!("          |                       |   ");
    println!("          V                       V   ");

    // Circle for P2ID Note
    println!("          ______________     ______________");
    println!("         /              \\   /              \\");
    println!("        |  P2ID Note    |  |  SWAPp'  Note |");
    println!(
        "        |  {} tokens B   |  |               |",
        amount_b_in.clone()
    );
    println!(
        "        |               |  |  {} tokens A |",
        format_value_to_float(expected_token_a_amount_remaining, 8)
    );
    println!("         \\______________/  |     for       |");
    println!(
        "                           | {} tokens B   |",
        format_value_to_float(requested_token_b_amount_remaining, 8)
    );
    println!("                            \\_____________/");
}

// @dev Test that a SWAPp note can be filled with the entire amount of the requested asset
#[test]
fn test_complete_swapp_fill() {
    // ASSETS
    // Offered Asset
    let faucet_id_1 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let amount_token_a: u64 = format_value_with_decimals(100, 8);
    let offered_token_a: Asset = FungibleAsset::new(faucet_id_1, amount_token_a)
        .unwrap()
        .into();

    // Requested Asset
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let requested_amount_token_b = format_value_with_decimals(200, 8);
    let requested_token_b: Asset = FungibleAsset::new(faucet_id_2, requested_amount_token_b)
        .unwrap()
        .into();

    // ACCOUNT IDs
    // SWAPp note creator
    let swapp_creator_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // SWAPp note consumer
    let swapp_consumer_account_id = AccountId::try_from(ACCOUNT_ID_SENDER_1).unwrap();

    // SWAPp note consumer wallet balance
    let token_b_amount_in = format_value_with_decimals(200, 8);
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

    // SWAPp note
    let (swap_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id.clone(),
        swapp_creator_account_id.clone(),
        offered_token_a,
        requested_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        0,
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
    let executed_transaction = executor
        .execute_transaction(
            swapp_consumer_account_id,
            block_ref,
            &note_ids,
            tx_args_target.clone(),
        )
        .unwrap();

    // P2ID & SWAPp note outputted by the transaction
    let p2id_ouput_note = executed_transaction.output_notes().get_note(0);

    // Calculate the amount of tokens A that are given to the consumer account
    let offered_token_a_out_amount =
        calculate_tokens_a_for_b(amount_token_a, requested_amount_token_b, token_b_amount_in);

    // Calculate the remaining tokens A and B
    let expected_token_a_amount_remaining = amount_token_a - offered_token_a_out_amount;
    let remaining_token_a: Asset =
        FungibleAsset::new(faucet_id_1, expected_token_a_amount_remaining)
            .unwrap()
            .into();

    let requested_token_b_amount_remaining = requested_amount_token_b - token_b_amount_in;
    let remaining_token_b: Asset =
        FungibleAsset::new(faucet_id_2, requested_token_b_amount_remaining)
            .unwrap()
            .into();

    // Note expected to be outputted by the transaction
    let (_expected_swapp_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id,
        swapp_consumer_account_id,
        remaining_token_a,
        remaining_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        0,
    )
    .unwrap();

    assert_eq!(executed_transaction.output_notes().num_notes(), 1);

    // @dev comment out to speed up test
    // assert!(prove_and_verify_transaction(executed_transaction.clone()).is_ok());

    let p2id_note_balance: &NoteAssets = p2id_ouput_note.assets().unwrap();

    let mut asset_iter = p2id_note_balance.iter();
    let p2id_note_asset = asset_iter.next().expect("Expected at least one asset");

    let token_b_output_in_p2id = match p2id_note_asset {
        Asset::Fungible(fa) => fa,
        _ => panic!("Expected a fungible asset, but found a non-fungible asset."),
    };

    let asset_id_in_p2id_note = token_b_output_in_p2id.faucet_id();
    let token_b_amount_in_p2id_note = token_b_output_in_p2id.amount();

    assert_eq!(p2id_note_balance.num_assets(), 1);

    assert_eq!(asset_id_in_p2id_note, faucet_id_2);
    assert_eq!(token_b_amount_in_p2id_note, token_b_amount_in);
}

// @dev Test that a SWAPp note can be partially filled by multiple users
#[test]
fn test_partial_swap_fill_multiple_consumers() {
    // ASSETS
    // Offered Asset
    let faucet_id_1 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let amount_token_a: u64 = format_value_with_decimals(100, 8);
    let offered_token_a: Asset = FungibleAsset::new(faucet_id_1, amount_token_a)
        .unwrap()
        .into();

    // Requested Asset
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let requested_amount_token_b = format_value_with_decimals(200, 8);
    let requested_token_b: Asset = FungibleAsset::new(faucet_id_2, requested_amount_token_b)
        .unwrap()
        .into();

    // ACCOUNT IDs
    // SWAPp note creator
    let swapp_creator_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // SWAPp note consumer 1
    let swapp_consumer_account_id = AccountId::try_from(ACCOUNT_ID_SENDER_1).unwrap();

    // SWAPp note consumer 1 wallet balance
    let swap_consumer_balance_token_b = format_value_with_decimals(120, 8);
    let swap_consumer_token_b = FungibleAsset::new(faucet_id_2, swap_consumer_balance_token_b)
        .unwrap()
        .into();
    let (target_pub_key_1, target_falcon_auth_1) = get_new_pk_and_authenticator();

    // SWAPp note consumer 1 wallet
    let swap_consumer_wallet = get_custom_account_code(
        swapp_consumer_account_id,
        target_pub_key_1,
        Some(swap_consumer_token_b),
    );

    let fill_number:u64 = 0;

    // Initial SWAPp note
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
        TransactionExecutor::new(tx_context.clone(), Some(target_falcon_auth_1.clone()))
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
    let executed_transaction = executor
        .execute_transaction(
            swapp_consumer_account_id,
            block_ref,
            &note_ids,
            tx_args_target.clone(),
        )
        .unwrap();

    // Note outputted by the transaction
    let swapp_output_note = executed_transaction.output_notes().get_note(1);

    // Calculate the amount of tokens A that are given to the consumer account
    let offered_token_a_out_amount = calculate_tokens_a_for_b(
        amount_token_a,
        requested_amount_token_b,
        swap_consumer_balance_token_b,
    );

    // Calculate the remaining tokens A and B
    let offered_token_a_amount_remaining = amount_token_a - offered_token_a_out_amount;
    let remaining_token_a: Asset =
        FungibleAsset::new(faucet_id_1, offered_token_a_amount_remaining)
            .unwrap()
            .into();

    let requested_token_b_amount_remaining =
        requested_amount_token_b - swap_consumer_balance_token_b;
    let remaining_token_b: Asset =
        FungibleAsset::new(faucet_id_2, requested_token_b_amount_remaining)
            .unwrap()
            .into();

    let fill_number_1: u64 = 1;

    // Note expected to be outputted by the transaction
    let (expected_swapp_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id,
        swapp_consumer_account_id,
        remaining_token_a,
        remaining_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        fill_number_1,
    )
    .unwrap();

    assert_eq!(executed_transaction.output_notes().num_notes(), 2);

    // Check that the output note is the same as the expected note
    assert_eq!(
        NoteHeader::from(swapp_output_note).metadata(),
        NoteHeader::from(expected_swapp_note.clone()).metadata()
    );

    assert_eq!(
        NoteHeader::from(swapp_output_note),
        NoteHeader::from(expected_swapp_note.clone())
    );

    // @dev comment out to speed up test
    // assert!(prove_and_verify_transaction(executed_transaction.clone()).is_ok());

    // CONSTRUCT AND EXECUTE TX 2 (Success)
    // --------------------------------------------------------------------------------------------

    // SWAPp note consumer 2
    let swapp_consumer_account_id_2 = AccountId::try_from(ACCOUNT_ID_SENDER_2).unwrap();

    // SWAPp note consumer 2 wallet balance
    let swap_consumer_2_balance_token_b = format_value_with_decimals(50, 8);
    let swap_consumer_token_b_1: Asset =
        FungibleAsset::new(faucet_id_2, swap_consumer_2_balance_token_b)
            .unwrap()
            .into();
    let (target_pub_key_2, target_falcon_auth_2) = get_new_pk_and_authenticator();

    // SWAPp note consumer 2 wallet
    let swap_consumer_wallet_1 = get_custom_account_code(
        swapp_consumer_account_id_2,
        target_pub_key_2,
        Some(swap_consumer_token_b_1),
    );

    let tx_context_1 = TransactionContextBuilder::new(swap_consumer_wallet_1.clone())
        .input_notes(vec![expected_swapp_note.clone()])
        .build();

    let mut executor = TransactionExecutor::new(tx_context_1.clone(), Some(target_falcon_auth_2))
        .with_debug_mode(true);
    executor.load_account(swapp_consumer_account_id_2).unwrap();

    let block_ref = tx_context_1.tx_inputs().block_header().block_num();
    let note_ids_1 = tx_context_1
        .tx_inputs()
        .input_notes()
        .iter()
        .map(|note| note.id())
        .collect::<Vec<_>>();

    // Execute the second transaction and get the witness
    let executed_transaction_1 = executor
        .execute_transaction(
            swapp_consumer_account_id_2,
            block_ref,
            &note_ids_1,
            tx_args_target.clone(),
        )
        .unwrap();

    // Note outputted by the transaction
    let swapp_output_note_1 = executed_transaction_1.output_notes().get_note(1);

    // Calculate the amount of tokens A that are given to the consumer account
    let offered_token_a_out_amount = calculate_tokens_a_for_b(
        offered_token_a_amount_remaining,
        requested_token_b_amount_remaining,
        swap_consumer_2_balance_token_b,
    );

    // Calculate the remaining tokens A and B
    let offered_token_a_amount_remaining_1 =
        offered_token_a_amount_remaining - offered_token_a_out_amount;
    let remaining_token_1: Asset =
        FungibleAsset::new(faucet_id_1, offered_token_a_amount_remaining_1)
            .unwrap()
            .into();

    let requested_token_b_amount_remaining_1 =
        requested_token_b_amount_remaining - swap_consumer_2_balance_token_b;
    let remaining_token_b_1: Asset =
        FungibleAsset::new(faucet_id_2, requested_token_b_amount_remaining_1)
            .unwrap()
            .into();

    let fill_number_2: u64 = 2;

    // Note expected to be outputted by the transaction
    let (expected_swapp_note_1, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id,
        swapp_consumer_account_id_2,
        remaining_token_1,
        remaining_token_b_1,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        fill_number_2,
    )
    .unwrap();

    assert_eq!(executed_transaction_1.output_notes().num_notes(), 2);

    // Check that the output note is the same as the expected note
    assert_eq!(
        NoteHeader::from(swapp_output_note_1).metadata(),
        NoteHeader::from(expected_swapp_note_1.clone()).metadata()
    );

    assert_eq!(
        NoteHeader::from(swapp_output_note_1),
        NoteHeader::from(expected_swapp_note_1.clone())
    );

    // @dev commented out to speed up test
    // assert!(prove_and_verify_transaction(executed_transaction.clone()).is_ok());
}

// @dev Test that a SWAPp note is reclaimable by the creator
#[test]
fn test_swap_reclaim() {
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
    let swap_consumer_balance_token_b = format_value_with_decimals(387, 6);
    let swap_consumer_token_b = FungibleAsset::new(faucet_id_2, swap_consumer_balance_token_b)
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
    let tx_result = executor
        .execute_transaction(
            swapp_consumer_account_id,
            block_ref,
            &note_ids,
            tx_args_target.clone(),
        )
        .unwrap();

    assert_eq!(tx_result.output_notes().num_notes(), 0);
    // assert!(prove_and_verify_transaction(tx_result.clone()).is_ok());
}

// @dev Test that a SWAPp note is reclaimable by the creator
#[test]
fn test_swap_zero_amount() {
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
    let swap_consumer_balance_token_b = format_value_with_decimals(0, 6);
    let swap_consumer_token_b = FungibleAsset::new(faucet_id_2, swap_consumer_balance_token_b)
        .unwrap()
        .into();
    let (target_pub_key, target_falcon_auth) = get_new_pk_and_authenticator();

    // SWAPp note consumer wallet
    let swap_consumer_wallet = get_custom_account_code(
        swapp_consumer_account_id,
        target_pub_key,
        Some(swap_consumer_token_b),
    );

    // SWAPp note
    let (swap_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id.clone(),
        swapp_creator_account_id.clone(),
        offered_token_a,
        requested_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        0,
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
    let tx_result = executor.execute_transaction(
        swapp_consumer_account_id,
        block_ref,
        &note_ids,
        tx_args_target.clone(),
    );

    assert!(tx_result.is_err());
}

// @dev Test swapping amount invalid note args amount
#[test]
fn test_swap_false_amount_via_note_args() {
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
    let swap_consumer_balance_token_b = format_value_with_decimals(0, 0);
    let swap_consumer_token_b = FungibleAsset::new(faucet_id_2, swap_consumer_balance_token_b)
        .unwrap()
        .into();
    let (target_pub_key, target_falcon_auth) = get_new_pk_and_authenticator();

    // SWAPp note consumer wallet
    let swap_consumer_wallet = get_custom_account_code(
        swapp_consumer_account_id,
        target_pub_key,
        Some(swap_consumer_token_b),
    );

    // SWAPp note
    let (swap_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id.clone(),
        swapp_creator_account_id.clone(),
        offered_token_a,
        requested_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        0,
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

    let invalid_note_args_amount = format_value_with_decimals(100, 6);

    // amount to consume
    let note_args = [[
        Felt::new(invalid_note_args_amount),
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
    ]];

    let note_args_map: BTreeMap<miden_client::notes::NoteId, [Felt; 4]> =
        BTreeMap::from([(note_ids[0], note_args[0])]);

    let tx_script_target = executor
        .compile_tx_script(tx_script_ast.clone(), vec![], vec![])
        .unwrap();

    let tx_args_target = TransactionArgs::new(
        Some(tx_script_target),
        Some(note_args_map),
        AdviceMap::default(),
    );

    // Execute the transaction and get the witness
    let tx_result = executor.execute_transaction(
        swapp_consumer_account_id,
        block_ref,
        &note_ids,
        tx_args_target.clone(),
    );

    assert!(tx_result.is_err());
}

// @dev Test that a SWAPp note consumer can specify the amount to consume via note args
#[test]
fn test_partial_swap_fill_with_note_args() {
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
    let swap_consumer_balance_token_b = format_value_with_decimals(387, 6);
    let swap_consumer_token_b = FungibleAsset::new(faucet_id_2, swap_consumer_balance_token_b)
        .unwrap()
        .into();
    let (target_pub_key, target_falcon_auth) = get_new_pk_and_authenticator();

    // SWAPp note consumer wallet
    let swap_consumer_wallet = get_custom_account_code(
        swapp_consumer_account_id,
        target_pub_key,
        Some(swap_consumer_token_b),
    );

    // SWAPp note
    let (swap_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id.clone(),
        swapp_creator_account_id.clone(),
        offered_token_a,
        requested_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        0,
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

    // amount to consume
    let note_args = [[
        Felt::new(swap_consumer_balance_token_b),
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
    ]];

    let note_args_map = BTreeMap::from([(note_ids[0], note_args[0])]);

    let tx_args_target = TransactionArgs::new(
        Some(tx_script_target),
        Some(note_args_map),
        AdviceMap::default(),
    );

    // Execute the transaction and get the witness
    let executed_transaction = executor
        .execute_transaction(
            swapp_consumer_account_id,
            block_ref,
            &note_ids,
            tx_args_target.clone(),
        )
        .unwrap();

    // SWAPp note outputted by the transaction
    let swapp_output_note = executed_transaction.output_notes().get_note(1);

    // Calculate the amount of tokens A that are given to the consumer account
    let offered_token_a_out_amount = calculate_tokens_a_for_b(
        amount_token_a,
        requested_amount_token_b,
        swap_consumer_balance_token_b,
    );

    // Calculate the remaining tokens A and B
    let offered_token_a_amount_remaining = amount_token_a - offered_token_a_out_amount;
    let remaining_token_a: Asset =
        FungibleAsset::new(faucet_id_1, offered_token_a_amount_remaining)
            .unwrap()
            .into();

    let requested_token_b_amount_remaining =
        requested_amount_token_b - swap_consumer_balance_token_b;
    let remaining_token_b: Asset =
        FungibleAsset::new(faucet_id_2, requested_token_b_amount_remaining)
            .unwrap()
            .into();

    // Note expected to be outputted by the transaction
    let (expected_swapp_note, _payback_note, _note_script_hash) = create_partial_swap_note(
        swapp_creator_account_id,
        swapp_consumer_account_id,
        remaining_token_a,
        remaining_token_b,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        0,
    )
    .unwrap();

    assert_eq!(executed_transaction.output_notes().num_notes(), 2);

    // Check that the output note is the same as the expected note
    assert_eq!(
        NoteHeader::from(swapp_output_note).metadata(),
        NoteHeader::from(expected_swapp_note.clone()).metadata()
    );

    assert_eq!(
        NoteHeader::from(swapp_output_note),
        NoteHeader::from(expected_swapp_note.clone())
    );
}

// @dev Demonstrate how to get the note script hash of the SWAPp note
#[test]
pub fn get_note_script_hash() {
    // SWAPp note creator
    let sender_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // Offered Asset
    let faucet_id = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let offered_asset: Asset = FungibleAsset::new(faucet_id, 100).unwrap().into();

    // Requested Asset
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let requested_asset: Asset = FungibleAsset::new(faucet_id_2, 100).unwrap().into();

    let (swap_note, _payback_note, note_script_hash) = create_partial_swap_note(
        sender_account_id,
        sender_account_id,
        offered_asset,
        requested_asset,
        NoteType::OffChain,
        [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)],
        0,
    )
    .unwrap();

    let tag = swap_note.clone().metadata().tag();
    let note_type = swap_note.clone().metadata().note_type();

    println!("{:?}", tag);
    println!("{:?}", note_type);
    println!("Note script hash: {:?}", note_script_hash);
}
