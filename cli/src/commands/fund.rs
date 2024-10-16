use std::{thread::sleep, time::Duration};

use clap::Parser;

use miden_client::{
    accounts::AccountId, assets::FungibleAsset, auth::TransactionAuthenticator, crypto::FeltRng,
    notes::NoteType, rpc::NodeRpcClient, store::Store, transactions::request::TransactionRequest,
    Client,
};

#[derive(Debug, Clone, Parser)]
#[clap(about = "Fund an account with test tokens")]
pub struct FundCmd {
    /// Account id to be funded
    account_id: String,

    /// Faucet id of the tokens
    faucet_id: String,

    /// Amount of tokens to be funded
    amount: u64,
}

impl FundCmd {
    pub async fn execute<N: NodeRpcClient, R: FeltRng, S: Store, A: TransactionAuthenticator>(
        &self,
        mut client: Client<N, R, S, A>,
    ) -> Result<(), String> {
        let account_id = AccountId::from_hex(self.account_id.as_str()).unwrap();
        let faucet_id = AccountId::from_hex(self.faucet_id.as_str()).unwrap();
        let (account, _) = client.get_account(account_id).unwrap();

        // Mint fungible asset
        let note_type = NoteType::Public;
        let asset = FungibleAsset::new(faucet_id, self.amount).unwrap();
        let transaction_request =
            TransactionRequest::mint_fungible_asset(asset, account.id(), note_type, client.rng())
                .unwrap();
        let tx_result = client
            .new_transaction(faucet_id, transaction_request)
            .unwrap();
        let asset_note_id = tx_result.relevant_notes()[0].id();
        client.submit_transaction(tx_result).await.unwrap();

        // Sync rollup state
        sleep(Duration::from_secs(20));
        client.sync_state().await?;

        // Fund account with asset
        let tx_request = TransactionRequest::consume_notes(vec![asset_note_id]);
        let tx_result = client.new_transaction(account_id, tx_request).unwrap();
        client.submit_transaction(tx_result).await.unwrap();

        println!("Account successfully funded.");

        Ok(())
    }
}
