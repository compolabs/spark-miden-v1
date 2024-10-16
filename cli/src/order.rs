use miden_client::{assets::Asset, notes::NoteId, store::InputNoteRecord};

use crate::{errors::OrderError, utils::get_assets_from_swap_note};

#[derive(Debug)]
pub struct Order {
    id: Option<NoteId>,
    source_asset: Asset,
    target_asset: Asset,
}

impl Order {
    pub fn new(id: Option<NoteId>, source_asset: Asset, target_asset: Asset) -> Self {
        Order {
            id,
            source_asset,
            target_asset,
        }
    }

    pub fn id(&self) -> Option<NoteId> {
        self.id
    }

    pub fn source_asset(&self) -> Asset {
        self.source_asset
    }

    pub fn target_asset(&self) -> Asset {
        self.target_asset
    }

    pub fn price(&self) -> f64 {
        let source_asset_amount = self.source_asset.unwrap_fungible().amount();
        let target_asset_amount = self.target_asset.unwrap_fungible().amount();

        target_asset_amount as f64 / source_asset_amount as f64
    }
}

// Conversion Into

impl From<InputNoteRecord> for Order {
    fn from(value: InputNoteRecord) -> Self {
        let (source_asset, target_asset) = get_assets_from_swap_note(&value);
        let id = value.id();
        Order {
            id: Some(id),
            source_asset,
            target_asset,
        }
    }
}

// Utils
/////////////////////////////////////////////////

pub fn match_orders(incoming_order: &Order, existing_order: &Order) -> Result<(), OrderError> {
    // Check that the assets are inversed
    if existing_order.source_asset.faucet_id() != incoming_order.target_asset.faucet_id()
        || existing_order.target_asset.faucet_id() != incoming_order.source_asset.faucet_id()
    {
        return Err(OrderError::AssetsNotMatching);
    }

    // Extract the amounts from the orders
    let existing_source_amount = existing_order.source_asset.unwrap_fungible().amount() as f64;
    let existing_target_amount = existing_order.target_asset.unwrap_fungible().amount() as f64;
    let incoming_source_amount = incoming_order.source_asset.unwrap_fungible().amount() as f64;
    let incoming_target_amount = incoming_order.target_asset.unwrap_fungible().amount() as f64;

    // Calculate the price of the existing order (price per unit of source asset)
    let existing_price = existing_target_amount / existing_source_amount;

    // Calculate the price of the incoming order
    let incoming_price = incoming_target_amount / incoming_source_amount;

    // Log the prices for debugging
    println!(
        "Existing order price: {}, Incoming order price: {}",
        existing_price, incoming_price
    );

    // Check if the existing order's price is equal to or lower than the incoming order's price
    if existing_price > incoming_price {
        panic!("price too high");
    }

    // Determine the amounts that can be traded (allow partial fills)
    let trade_source_amount = existing_source_amount.min(incoming_target_amount);
    let trade_target_amount = existing_target_amount.min(incoming_source_amount);

    // Ensure trade amounts are positive
    if trade_source_amount <= 0.0 || trade_target_amount <= 0.0 {
        panic!("must be positive");
    }

    // Orders can be matched
    Ok(())
}

// Tests
/////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use miden_client::{
        accounts::AccountId,
        assets::{Asset, FungibleAsset},
        notes::NoteId,
    };

    use crate::{errors::OrderError, order::match_orders};

    use super::Order;

    fn mock_orders() -> (Order, Vec<Order>) {
        // create faucets
        let source_faucet_id_hex = "0x227bd163275aa1bf";
        let source_faucet_id = AccountId::from_hex(source_faucet_id_hex).unwrap();
        let target_faucet_id_hex = "0x2540b08edc3b087d";
        let target_faucet_id = AccountId::from_hex(target_faucet_id_hex).unwrap();

        // mock note id
        let note_id_hex = "0x17c0bee79464320cc0d5d835cb9c2971b5c23fcea665c66d4f73c54fc7860129";
        let note_id = NoteId::try_from_hex(note_id_hex).unwrap();

        // create assets
        let source_amount = 10;
        let target_amount = 20;
        let source_asset =
            Asset::Fungible(FungibleAsset::new(source_faucet_id, source_amount).unwrap());
        let target_asset =
            Asset::Fungible(FungibleAsset::new(target_faucet_id, target_amount).unwrap());

        // incoming order
        let order = Order::new(Some(note_id), source_asset, target_asset);

        // existing orders

        // Perfect match
        let order1 = Order::new(Some(note_id), target_asset, source_asset);

        // Assets do not match
        let order2 = Order::new(Some(note_id), source_asset, source_asset);

        // Not enough target assets
        let new_target_amount = 19;
        let new_target_asset =
            Asset::Fungible(FungibleAsset::new(target_faucet_id, new_target_amount).unwrap());
        let order3 = Order::new(Some(note_id), new_target_asset, source_asset);

        // Too many requested assets
        let new_source_amount = 11;
        let new_source_asset =
            Asset::Fungible(FungibleAsset::new(source_faucet_id, new_source_amount).unwrap());
        let order4 = Order::new(Some(note_id), target_asset, new_source_asset);

        // Acceptable match
        let new_target_amount = 200;
        let new_target_asset =
            Asset::Fungible(FungibleAsset::new(target_faucet_id, new_target_amount).unwrap());
        let order5 = Order::new(Some(note_id), new_target_asset, source_asset);

        let orders = vec![order1, order2, order3, order4, order5];

        (order, orders)
    }

    #[test]
    fn order_matching_succeeds() {
        let (incoming_order, existing_orders) = mock_orders();
        let expected_results = [
            Ok(()),
            Err(OrderError::AssetsNotMatching),
            Err(OrderError::TooFewSourceAssets),
            Err(OrderError::TooManyTargetAssets),
            Ok(()),
        ];

        for (existing_order, expected_result) in existing_orders.into_iter().zip(expected_results) {
            assert_eq!(
                match_orders(&incoming_order, &existing_order),
                expected_result,
                "Mismatch for order: {:?}",
                existing_order
            );
        }
    }
}
