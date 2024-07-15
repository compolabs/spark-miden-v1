use std::{env::temp_dir, rc::Rc, time::Duration};

use figment::{
    providers::{Format, Toml},
    Figment,
};
use miden_client::{
    accounts::AccountTemplate,
    auth::StoreAuthenticator,
    config::RpcConfig,
    rpc::{RpcError, TonicRpcClient},
    store::{
        sqlite_store::{config::SqliteStoreConfig, SqliteStore},
        NoteFilter, TransactionFilter,
    },
    sync::SyncSummary,
    transactions::{
        transaction_request::{TransactionRequest, TransactionTemplate},
        DataStoreError, TransactionExecutorError,
    },
    Client, ClientError,
};
use miden_objects::{
    accounts::{
        account_id::testing::ACCOUNT_ID_REGULAR_ACCOUNT_UPDATABLE_CODE_OFF_CHAIN, Account,
        AccountId, AccountStorageType,
    },
    assets::{Asset, FungibleAsset, TokenSymbol},
    crypto::rand::RpoRandomCoin,
    notes::{NoteId, NoteType},
    transaction::{InputNote, TransactionId},
    Felt,
};
use rand::Rng;
use uuid::Uuid;

pub const ACCOUNT_ID_REGULAR: u64 = ACCOUNT_ID_REGULAR_ACCOUNT_UPDATABLE_CODE_OFF_CHAIN;

pub type TestClient = Client<
    TonicRpcClient,
    RpoRandomCoin,
    SqliteStore,
    StoreAuthenticator<RpoRandomCoin, SqliteStore>,
>;

pub const TEST_CLIENT_RPC_CONFIG_FILE_PATH: &str = "./tests/config/miden-client-rpc.toml";
/// Creates a `TestClient`
///
/// Creates the client using the config at `TEST_CLIENT_CONFIG_FILE_PATH`. The store's path is at a random temporary location, so the store section of the config file is ignored.
///
/// # Panics
///
/// Panics if there is no config file at `TEST_CLIENT_CONFIG_FILE_PATH`, or it cannot be
/// deserialized into a [ClientConfig]
pub fn create_test_client() -> TestClient {
    let (rpc_config, store_config) = get_client_config();

    let store = {
        let sqlite_store = SqliteStore::new(&store_config).unwrap();
        Rc::new(sqlite_store)
    };

    let mut rng = rand::thread_rng();
    let coin_seed: [u64; 4] = rng.gen();

    let rng = RpoRandomCoin::new(coin_seed.map(Felt::new));

    let authenticator = StoreAuthenticator::new_with_rng(store.clone(), rng);
    TestClient::new(
        TonicRpcClient::new(&rpc_config),
        rng,
        store,
        authenticator,
        true,
    )
}

pub fn get_client_config() -> (RpcConfig, SqliteStoreConfig) {
    let rpc_config: RpcConfig = Figment::from(Toml::file(TEST_CLIENT_RPC_CONFIG_FILE_PATH))
        .extract()
        .expect("should be able to read test config at {TEST_CLIENT_CONFIG_FILE_PATH}");

    let store_config = create_test_store_path()
        .into_os_string()
        .into_string()
        .unwrap()
        .try_into()
        .unwrap();

    (rpc_config, store_config)
}

pub fn create_test_store_path() -> std::path::PathBuf {
    let mut temp_file = temp_dir();
    temp_file.push(format!("{}.sqlite3", Uuid::new_v4()));
    temp_file
}

pub async fn execute_tx(client: &mut TestClient, tx_request: TransactionRequest) -> TransactionId {
    println!("Executing transaction...");
    let transaction_execution_result = client.new_transaction(tx_request).unwrap();
    let transaction_id = transaction_execution_result.executed_transaction().id();

    println!("Sending transaction to node");
    let proven_transaction = client
        .prove_transaction(transaction_execution_result.executed_transaction().clone())
        .unwrap();
    client
        .submit_transaction(transaction_execution_result, proven_transaction)
        .await
        .unwrap();

    transaction_id
}

pub async fn execute_tx_and_sync(client: &mut TestClient, tx_request: TransactionRequest) {
    let transaction_id = execute_tx(client, tx_request).await;
    wait_for_tx(client, transaction_id).await;
}

pub async fn wait_for_tx(client: &mut TestClient, transaction_id: TransactionId) {
    // wait until tx is committed
    loop {
        println!("Syncing State...");
        client.sync_state().await.unwrap();

        // Check if executed transaction got committed by the node
        let uncommited_transactions = client
            .get_transactions(TransactionFilter::Uncomitted)
            .unwrap();
        let is_tx_committed = uncommited_transactions
            .iter()
            .all(|uncommited_tx| uncommited_tx.id != transaction_id);

        if is_tx_committed {
            break;
        }

        std::thread::sleep(std::time::Duration::new(3, 0));
    }
}

// Syncs until `amount_of_blocks` have been created onchain compared to client's sync height
pub async fn wait_for_blocks(client: &mut TestClient, amount_of_blocks: u32) -> SyncSummary {
    let current_block = client.get_sync_height().unwrap();
    let final_block = current_block + amount_of_blocks;
    println!("Syncing until block {}...", final_block);
    // wait until tx is committed
    loop {
        let summary = client.sync_state().await.unwrap();
        println!(
            "Synced to block {} (syncing until {})...",
            summary.block_num, final_block
        );

        if summary.block_num >= final_block {
            return summary;
        }

        std::thread::sleep(std::time::Duration::new(3, 0));
    }
}

/// Waits for node to be running.
///
/// # Panics
///
/// This function will panic if it does `NUMBER_OF_NODE_ATTEMPTS` unsuccessful checks or if we
/// receive an error other than a connection related error
pub async fn wait_for_node(client: &mut TestClient) {
    const NODE_TIME_BETWEEN_ATTEMPTS: u64 = 5;
    const NUMBER_OF_NODE_ATTEMPTS: u64 = 60;

    println!("Waiting for Node to be up. Checking every {NODE_TIME_BETWEEN_ATTEMPTS}s for {NUMBER_OF_NODE_ATTEMPTS} tries...");

    for _try_number in 0..NUMBER_OF_NODE_ATTEMPTS {
        match client.sync_state().await {
            Err(ClientError::RpcError(RpcError::ConnectionError(_))) => {
                std::thread::sleep(Duration::from_secs(NODE_TIME_BETWEEN_ATTEMPTS));
            }
            Err(other_error) => {
                panic!("Unexpected error: {other_error}");
            }
            _ => return,
        }
    }

    panic!("Unable to connect to node");
}

pub const MINT_AMOUNT: u64 = 1000;
pub const TRANSFER_AMOUNT: u64 = 59;

/// Sets up a basic client and returns (basic_account, basic_account, faucet_account)
pub async fn setup(
    client: &mut TestClient,
    accounts_storage_mode: AccountStorageType,
) -> (Account, Account, Account) {
    // Enusre clean state
    assert!(client.get_account_stubs().unwrap().is_empty());
    assert!(client
        .get_transactions(TransactionFilter::All)
        .unwrap()
        .is_empty());
    assert!(client.get_input_notes(NoteFilter::All).unwrap().is_empty());

    // Create faucet account
    let (faucet_account, _) = client
        .new_account(AccountTemplate::FungibleFaucet {
            token_symbol: TokenSymbol::new("MATIC").unwrap(),
            decimals: 8,
            max_supply: 1_000_000_000,
            storage_type: accounts_storage_mode,
        })
        .unwrap();

    // Create regular accounts
    let (first_basic_account, _) = client
        .new_account(AccountTemplate::BasicWallet {
            mutable_code: false,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();

    let (second_basic_account, _) = client
        .new_account(AccountTemplate::BasicWallet {
            mutable_code: false,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();

    println!("Syncing State...");
    client.sync_state().await.unwrap();

    // Get Faucet and regular accounts
    println!("Fetching Accounts...");
    (first_basic_account, second_basic_account, faucet_account)
}

pub async fn setup_with_tokens(
    client: &mut TestClient,
) -> (Account, FungibleAsset, Account, FungibleAsset) {
    // Ensure clean state
    assert!(client.get_account_stubs().unwrap().is_empty());
    assert!(client
        .get_transactions(TransactionFilter::All)
        .unwrap()
        .is_empty());
    assert!(client.get_input_notes(NoteFilter::All).unwrap().is_empty());

    // Create account A with tokenA
    let (account_a, asset_a) = create_account_with_token(client, "TOKA", 8, 1_000_000_000).await;

    // Create account B with tokenB
    let (account_b, asset_b) = create_account_with_token(client, "TOKB", 8, 1_000_000_000).await;

    println!("Syncing State...");
    client.sync_state().await.unwrap();

    // Return the created accounts and their respective tokens
    (account_a, asset_a, account_b, asset_b)
}

async fn create_account_with_token(
    client: &mut TestClient,
    token_symbol: &str,
    decimals: u8,
    max_supply: u64,
) -> (Account, FungibleAsset) {
    let account_template = AccountTemplate::FungibleFaucet {
        token_symbol: TokenSymbol::new(token_symbol).unwrap(),
        decimals,
        max_supply,
        storage_type: AccountStorageType::OffChain,
    };
    client.sync_state().await.unwrap();
    let (account, _) = client.new_account(account_template).unwrap();
    let asset = FungibleAsset::new(account.id(), max_supply).unwrap();
    (account, asset)
}

/// Mints a note from faucet_account_id for basic_account_id, waits for inclusion and returns it
/// with 1000 units of the corresponding fungible asset
pub async fn mint_note(
    client: &mut TestClient,
    basic_account_id: AccountId,
    faucet_account_id: AccountId,
    note_type: NoteType,
) -> InputNote {
    // Create a Mint Tx for 1000 units of our fungible asset
    let fungible_asset = FungibleAsset::new(faucet_account_id, MINT_AMOUNT).unwrap();
    let tx_template =
        TransactionTemplate::MintFungibleAsset(fungible_asset, basic_account_id, note_type);

    println!("Minting Asset");
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(client, tx_request.clone()).await;

    // Check that note is committed and return it
    println!("Fetching Committed Notes...");
    let note_id = tx_request.expected_output_notes()[0].id();
    let note = client.get_input_note(note_id).unwrap();
    note.try_into().unwrap()
}

/// Consumes and wait until the transaction gets committed
/// This assumes the notes contain assets
pub async fn consume_notes(
    client: &mut TestClient,
    account_id: AccountId,
    input_notes: &[InputNote],
) {
    let tx_template =
        TransactionTemplate::ConsumeNotes(account_id, input_notes.iter().map(|n| n.id()).collect());
    println!("Consuming Note...");
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(client, tx_request).await;
}

pub async fn assert_account_has_single_asset(
    client: &TestClient,
    account_id: AccountId,
    asset_account_id: AccountId,
    expected_amount: u64,
) {
    let (regular_account, _seed) = client.get_account(account_id).unwrap();

    assert_eq!(regular_account.vault().assets().count(), 1);
    let asset = regular_account.vault().assets().next().unwrap();

    if let Asset::Fungible(fungible_asset) = asset {
        assert_eq!(fungible_asset.faucet_id(), asset_account_id);
        assert_eq!(fungible_asset.amount(), expected_amount);
    } else {
        panic!("Account has consumed a note and should have a fungible asset");
    }
}

pub async fn assert_note_cannot_be_consumed_twice(
    client: &mut TestClient,
    consuming_account_id: AccountId,
    note_to_consume_id: NoteId,
) {
    // Check that we can't consume the P2ID note again
    let tx_template =
        TransactionTemplate::ConsumeNotes(consuming_account_id, vec![note_to_consume_id]);
    println!("Consuming Note...");

    // Double-spend error expected to be received since we are consuming the same note
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    match client.new_transaction(tx_request) {
        Err(ClientError::TransactionExecutorError(
            TransactionExecutorError::FetchTransactionInputsFailed(
                DataStoreError::NoteAlreadyConsumed(_),
            ),
        )) => {}
        Ok(_) => panic!("Double-spend error: Note should not be consumable!"),
        err => panic!(
            "Unexpected error {:?} for note ID: {}",
            err,
            note_to_consume_id.to_hex()
        ),
    }
}
