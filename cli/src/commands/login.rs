use crate::utils::load_accounts;
use clap::Parser;

use miden_client::{
    accounts::{AccountStorageType, AccountTemplate},
    auth::TransactionAuthenticator,
    crypto::FeltRng,
    rpc::NodeRpcClient,
    store::Store,
    Client,
};

#[derive(Debug, Clone, Parser)]
#[clap(about = "Create a new account and login")]
pub struct LoginCmd {}

impl LoginCmd {
    pub fn execute<N: NodeRpcClient, R: FeltRng, S: Store, A: TransactionAuthenticator>(
        &self,
        mut client: Client<N, R, S, A>,
    ) -> Result<(), String> {
        // Import existing accounts
        let accounts = load_accounts().unwrap();
        for account_data in accounts {
            println!("account: {:?}", account_data.account.id().to_hex());

            for asset in account_data.account.vault().assets() {
                println!("Asset: {:?}", asset);
            }

            client.import_account(account_data).unwrap()
        }

        // Create user account
        let wallet_template = AccountTemplate::BasicWallet {
            mutable_code: false,
            storage_type: AccountStorageType::OnChain,
        };

        let (account, _) = client
            .new_account(wallet_template)
            .map_err(|e| e.to_string())?;

        println!("Successful login, account id: {}", account.id());

        for asset in account.vault().assets() {
            println!("Asset: {:?}", asset);
        }

        Ok(())
    }
}
