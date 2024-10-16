use miden_client::{
    accounts::AccountId,
    assets::{Asset, FungibleAsset},
    auth::TransactionAuthenticator,
    crypto::FeltRng,
    notes::NoteType,
    rpc::NodeRpcClient,
    store::Store,
    transactions::{build_swap_tag, request::TransactionRequest},
    Client,
};

use clap::Parser;

use crate::{
    order::{match_orders, Order},
    utils::{get_notes_by_tag, print_order_table, sort_orders},
};

use crate::utils::{compute_p2id_serial_num, create_p2id_note, create_partial_swap_note};
use miden_objects::Felt;

#[derive(Debug, Clone, Parser)]
#[command(about = "Execute an order")]
pub struct OrderCmd {
    /// Account executing the order
    account_id: String,

    /// Target faucet id
    target_faucet: String,

    /// Target asset amount
    target_amount: u64,

    /// Source faucet id
    source_faucet: String,

    /// Source asset amount
    source_amount: u64,
}

impl OrderCmd {
    pub async fn execute<N: NodeRpcClient, R: FeltRng, S: Store, A: TransactionAuthenticator>(
        &self,
        mut client: Client<N, R, S, A>,
    ) -> Result<(), String> {
        // Parse id's
        let _account_id = AccountId::from_hex(self.account_id.as_str()).unwrap();
        let source_faucet_id = AccountId::from_hex(self.source_faucet.as_str()).unwrap();
        let target_faucet_id = AccountId::from_hex(self.target_faucet.as_str()).unwrap();

        // Build order
        let source_asset =
            Asset::Fungible(FungibleAsset::new(source_faucet_id, self.source_amount).unwrap());
        let target_asset =
            Asset::Fungible(FungibleAsset::new(target_faucet_id, self.target_amount).unwrap());
        let incoming_order = Order::new(None, source_asset, target_asset);

        // Get relevant notes
        let tag = build_swap_tag(NoteType::Public, target_faucet_id, source_faucet_id).unwrap();
        let notes: Vec<miden_client::store::InputNoteRecord> = get_notes_by_tag(&client, tag);

        assert!(!notes.is_empty(), "There are no relevant orders available.");

        // find matching orders
        let matching_orders: Vec<Order> = notes
            .into_iter()
            .map(Order::from)
            .filter(|order| match_orders(&incoming_order, order).is_ok())
            .collect();
        let sorted_orders = sort_orders(matching_orders);

        println!("sorted orders: {:?}", sorted_orders);

        print_order_table(&sorted_orders);

        let swap_note_order = sorted_orders.first().unwrap();

        let swap_note = client
            .get_input_note(swap_note_order.id().unwrap())
            .unwrap();

        println!("account id consume: {:?}", _account_id);
        println!("source asset: {:?}", source_faucet_id);
        println!("target asset: {:?}", target_faucet_id);
        println!("swap inputs: {:?}", swap_note.details().inputs());
        println!("swap metadata: {:?}", swap_note.metadata());
        println!("swap asset: {:?}", swap_note.assets());

        let creator: AccountId =
            match AccountId::try_from(swap_note.details().inputs().get(12).unwrap().as_int()) {
                Ok(account_id) => account_id,
                Err(e) => {
                    panic!("Failed to convert to AccountId: {:?}", e);
                }
            };

        let swap_serial_num = swap_note.details().serial_num();
        let fill_number = swap_note.details().inputs().get(8).unwrap().as_int();
        let next_fill_number = fill_number + 1;

        let offered_remaining: Asset = FungibleAsset::new(target_faucet_id, 5).unwrap().into();
        let requested_remaining: Asset = FungibleAsset::new(source_faucet_id, 5).unwrap().into();

        let requested_filled: Asset = FungibleAsset::new(source_faucet_id, 5).unwrap().into();

        let output_swap_note = create_partial_swap_note(
            creator,
            _account_id,
            offered_remaining,
            requested_remaining,
            swap_serial_num,
            next_fill_number,
        )
        .unwrap();

        let p2id_serial_num = compute_p2id_serial_num(swap_serial_num, next_fill_number);

        let expected_p2id_note = create_p2id_note(
            _account_id,
            creator,
            vec![requested_filled],
            NoteType::Public,
            Felt::new(0),
            p2id_serial_num,
        )
        .unwrap();

        let tx_request: TransactionRequest = TransactionRequest::new()
            .with_authenticated_input_notes([(swap_note.id(), None)])
            .with_expected_output_notes(vec![expected_p2id_note, output_swap_note]);

        println!("Executing transaction...");
        let transaction_execution_result = client.new_transaction(_account_id, tx_request).unwrap();
        client
            .submit_transaction(transaction_execution_result)
            .await
            .unwrap();

        // // find matching orders
        // let matching_order_ids: Result<Vec<NoteId>, OrderError> = relevant_notes
        //     .into_iter()
        //     .map(Order::from)
        //     .filter(|order| match_orders(&incoming_order, order).is_ok())
        //     .map(|matching_order| matching_order.id().ok_or(OrderError::MissingOrderId))
        //     .collect();

        // // Create transaction
        /*         let transaction_request = TransactionRequest::consume_notes(matching_order_ids);

               let transaction = client
                   .new_transaction(account_id, transaction_request)
                   .map_err(|e| format!("Failed to create transaction: {}", e))?;

               client
                   .submit_transaction(transaction)
                   .await
                   .map_err(|e| format!("Failed to submit transaction: {}", e))?;
        */
        Ok(())
    }
}
