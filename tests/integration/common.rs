use std::{
    collections::BTreeMap,
    env::temp_dir,
    rc::Rc,
    time::Duration,
};

use figment::{
    providers::{Format, Toml},
    Figment,
};

use miden_client::{
    accounts::{Account, AccountId, AccountStorageType, AccountTemplate},
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

use miden_lib::{
    notes::utils::build_p2id_recipient,
    transaction::TransactionKernel,
};

use miden_objects::{
    assets::{Asset, FungibleAsset, TokenSymbol},
    assembly::AssemblyContext,
    crypto::rand::RpoRandomCoin,
    notes::{
        Note, NoteAssets, NoteExecutionHint, NoteId, NoteInputs, NoteMetadata,
        NoteRecipient, NoteScript, NoteTag, NoteType,
    },
    transaction::{InputNote, TransactionId},
    vm::CodeBlock,
    Felt, NoteError, Word, ZERO,
};

use miden_vm::{Assembler, ProgramAst};

use rand::Rng;
use uuid::Uuid;

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

pub async fn setup_with_tokens(client: &mut TestClient) -> (Account, Account, Account, Account) {
    // Ensure clean state
    assert!(client.get_account_stubs().unwrap().is_empty());
    assert!(client
        .get_transactions(TransactionFilter::All)
        .unwrap()
        .is_empty());
    assert!(client.get_input_notes(NoteFilter::All).unwrap().is_empty());

    // Create faucet account A
    let (asset_a, _) = client
        .new_account(AccountTemplate::FungibleFaucet {
            token_symbol: TokenSymbol::new("TOKA").unwrap(),
            decimals: 8,
            max_supply: 1_000_000_000,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();

    // Create faucet account A
    let (asset_b, _) = client
        .new_account(AccountTemplate::FungibleFaucet {
            token_symbol: TokenSymbol::new("TOKB").unwrap(),
            decimals: 8,
            max_supply: 1_000_000_000,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();

    // Create regular accounts
    let (account_a, _) = client
        .new_account(AccountTemplate::BasicWallet {
            mutable_code: false,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();

    let (account_b, _) = client
        .new_account(AccountTemplate::BasicWallet {
            mutable_code: false,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();

    // Return the created accounts and their respective tokens
    (account_a, account_b, asset_a, asset_b)
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

/// Mints a note from faucet_account_id for basic_account_id, waits for inclusion and returns it
/// with 1000 units of the corresponding fungible asset
pub async fn mint_note_with_amount(
    client: &mut TestClient,
    basic_account_id: AccountId,
    faucet_account_id: AccountId,
    faucet_mint_amount: u64,
    note_type: NoteType,
) -> InputNote {
    let fungible_asset = FungibleAsset::new(faucet_account_id, faucet_mint_amount).unwrap();
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

pub fn format_value_with_decimals(value: u64, decimals: u32) -> u64 {
    value * 10u64.pow(decimals)
}

pub fn format_value_to_float(value: u64, decimals: u32) -> f32 {
    let scale = 10f32.powi(decimals as i32);
    let result: f32 = ((value as f32) / scale) as f32;
    result
}

// Helper function to calculate tokens_a for tokens_b
pub fn calculate_tokens_a_for_b(tokens_a: u64, tokens_b: u64, token_b_amount_in: u64) -> u64 {
    let scaling_factor: u64 = 100_000;

    if tokens_a < tokens_b {
        let scaled_ratio = (tokens_b * scaling_factor) / tokens_a;
        (token_b_amount_in * scaling_factor) / scaled_ratio
    } else {
        let scaled_ratio = (tokens_a * scaling_factor) / tokens_b;
        (scaled_ratio * token_b_amount_in) / scaling_factor
    }
}

pub async fn print_account_balance(client: &TestClient, account_id: AccountId) {
    // Retrieve the account from the client
    let (regular_account, _seed) = client.get_account(account_id).unwrap();

    // Ensure the account has exactly one asset
    if regular_account.vault().assets().count() != 1 {
        println!("Account ID: {:?}", regular_account.id());
        println!(
            "number of assets {:?}",
            regular_account.vault().assets().count()
        );
        let assets: Vec<_> = regular_account.vault().assets().collect();
        for asset in &assets {
            println!("Asset {:?}", asset);
        }
        panic!("Account does not have exactly one asset");
    }

    // Get the asset from the account
    let asset = regular_account.vault().assets().next().unwrap();

    // Match on the asset to handle different types
    if let Asset::Fungible(fungible_asset) = asset {
        // Print the details of the fungible asset
        println!("User Account ID: {:?}", account_id);
        println!("token faucet ID: {:?}", fungible_asset.faucet_id());
        println!("Amount: {}", fungible_asset.amount());
    } else {
        panic!("Account has an unexpected asset type");
    }
}

pub fn new_note_script(
    code: ProgramAst,
    assembler: &Assembler,
) -> Result<(NoteScript, CodeBlock), NoteError> {
    // Compile the code in the context with phantom calls enabled
    let code_block = assembler
        .compile_in_context(
            &code,
            &mut AssemblyContext::for_program(Some(&code)).with_phantom_calls(true),
        )
        .map_err(NoteError::ScriptCompilationError)?;

    // Use the from_parts method to create a NoteScript instance
    let note_script = NoteScript::from_parts(code, code_block.hash());

    Ok((note_script, code_block))
}

pub async fn create_partial_swap_note(
    client: &mut TestClient,
    sender: AccountId,
    last_consumer: AccountId,
    offered_asset_id: AccountId,
    offered_asset_amount: u64,
    requested_asset_id: AccountId,
    requested_asset_amount: u64,
    note_type: NoteType,
    serial_num: [Felt; 4],
) -> Result<Note, NoteError> {
    let note_code = include_str!("../../src/notes/SWAPp.masm");
    let (note_script, _code_block) = new_note_script(
        ProgramAst::parse(note_code).unwrap(),
        &TransactionKernel::assembler().with_debug_mode(true),
    )
    .unwrap();

    let offered_asset: Asset =
        Asset::from(FungibleAsset::new(offered_asset_id, offered_asset_amount).unwrap());
    let requested_asset: Asset =
        Asset::from(FungibleAsset::new(requested_asset_id, requested_asset_amount).unwrap());

    let payback_recipient = build_p2id_recipient(sender, serial_num)?;

    let payback_recipient_word: Word = payback_recipient.digest().into();
    let requested_asset_word: Word = requested_asset.into();

    println!("script hash {:?}", note_script.hash());
    println!("requested asset id: {:?}", requested_asset.faucet_id());

    // build the tag for the SWAP use case
    let tag = build_swap_tag(note_type, &offered_asset, &requested_asset)?;

    let inputs = NoteInputs::new(vec![
        payback_recipient_word[0],
        payback_recipient_word[1],
        payback_recipient_word[2],
        payback_recipient_word[3],
        requested_asset_word[0],
        requested_asset_word[1],
        requested_asset_word[2],
        requested_asset_word[3],
        tag.inner().into(),
    ])?;

    let aux = ZERO;

    // build the outgoing note
    let metadata = NoteMetadata::new(last_consumer, note_type, tag, aux)?;
    let assets = NoteAssets::new(vec![offered_asset])?;
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), inputs.clone());
    let swap_note = Note::new(assets.clone(), metadata, recipient.clone());

    println!("Attempting to mint SWAPp note...");

    // note created, now let's mint it
    let recipient = swap_note
        .recipient()
        .digest()
        .iter()
        .map(|x| x.as_int().to_string())
        .collect::<Vec<_>>()
        .join(".");

    let code = "
    use.miden::contracts::auth::basic->auth_tx
    use.miden::contracts::wallets::basic->wallet
    use.std::sys

    begin
        push.{recipient}
        push.2
        push.0
        push.{tag}
        push.{amount}
        push.0.0
        push.{token_id}

        call.wallet::send_asset

        call.auth_tx::auth_tx_rpo_falcon512

        exec.sys::truncate_stack
    end
    "
    .replace("{recipient}", &recipient.clone())
    .replace("{tag}", &Felt::new(tag.clone().into()).to_string())
    .replace(
        "{amount}",
        &offered_asset.unwrap_fungible().amount().to_string(),
    )
    .replace(
        "{token_id}",
        &Felt::new(offered_asset.faucet_id().into()).to_string(),
    );

    let program = ProgramAst::parse(&code).unwrap();
    let tx_script = client.compile_tx_script(program, vec![], vec![]).unwrap();

    let transaction_request = TransactionRequest::new(
        sender,
        vec![],
        BTreeMap::new(),
        vec![swap_note.clone()],
        vec![],
        Some(tx_script),
        None,
    )
    .unwrap();

    println!("Attempting to create SWAPp note...");
    println!("SWAPp noteID {:?}", swap_note.id());

    let _ = execute_tx_and_sync(client, transaction_request).await;

    println!("SWAPp note created!");

    Ok(swap_note)
}

pub fn create_partial_swap_note_offchain(
    sender: AccountId,
    last_consumer: AccountId,
    offered_asset_id: AccountId,
    offered_asset_amount: u64,
    requested_asset_id: AccountId,
    requested_asset_amount: u64,
    note_type: NoteType,
    serial_num: [Felt; 4],
) -> Result<Note, NoteError> {
    let note_code = include_str!("../../src/notes/SWAPp.masm");
    let (note_script, _code_block) = new_note_script(
        ProgramAst::parse(note_code).unwrap(),
        &TransactionKernel::assembler().with_debug_mode(true),
    )
    .unwrap();

    let offered_asset: Asset =
        Asset::from(FungibleAsset::new(offered_asset_id, offered_asset_amount).unwrap());
    let requested_asset: Asset =
        Asset::from(FungibleAsset::new(requested_asset_id, requested_asset_amount).unwrap());

    let payback_recipient = build_p2id_recipient(sender, serial_num)?;

    let payback_recipient_word: Word = payback_recipient.digest().into();
    let requested_asset_word: Word = requested_asset.into();

    println!("script hash {:?}", note_script.hash());
    println!("requested asset id: {:?}", requested_asset.faucet_id());

    // build the tag for the SWAP use case
    let tag = build_swap_tag(note_type, &offered_asset, &requested_asset)?;

    let inputs = NoteInputs::new(vec![
        payback_recipient_word[0],
        payback_recipient_word[1],
        payback_recipient_word[2],
        payback_recipient_word[3],
        requested_asset_word[0],
        requested_asset_word[1],
        requested_asset_word[2],
        requested_asset_word[3],
        tag.inner().into(),
    ])?;

    let aux = ZERO;

    // build the outgoing note
    let metadata = NoteMetadata::new(last_consumer, note_type, tag, aux)?;
    let assets = NoteAssets::new(vec![offered_asset])?;
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), inputs.clone());
    let swap_note = Note::new(assets.clone(), metadata, recipient.clone());

    Ok(swap_note)
}

fn build_swap_tag(
    note_type: NoteType,
    offered_asset: &Asset,
    requested_asset: &Asset,
) -> Result<NoteTag, NoteError> {
    const SWAP_USE_CASE_ID: u16 = 0;

    // get bits 4..12 from faucet IDs of both assets, these bits will form the tag payload; the
    // reason we skip the 4 most significant bits is that these encode metadata of underlying
    // faucets and are likely to be the same for many different faucets.

    let offered_asset_id: u64 = offered_asset.faucet_id().into();
    let offered_asset_tag = (offered_asset_id >> 52) as u8;

    let requested_asset_id: u64 = requested_asset.faucet_id().into();
    let requested_asset_tag = (requested_asset_id >> 52) as u8;

    let payload = ((offered_asset_tag as u16) << 8) | (requested_asset_tag as u16);

    let execution = NoteExecutionHint::Local;
    match note_type {
        NoteType::Public => NoteTag::for_public_use_case(SWAP_USE_CASE_ID, payload, execution),
        _ => NoteTag::for_local_use_case(SWAP_USE_CASE_ID, payload),
    }
}

pub fn create_p2id_note_with_serial_num(
    sender: AccountId,
    target: AccountId,
    assets: Vec<Asset>,
    note_type: NoteType,
    aux: Felt,
    serial_num: [Felt; 4],
) -> Result<Note, NoteError> {
    let p2id_code = include_str!("../../src/notes/P2ID.masm");
    let (note_script, _codeblock) = new_note_script(
        ProgramAst::parse(p2id_code).unwrap(),
        &TransactionKernel::assembler().with_debug_mode(true),
    )
    .unwrap();
    let inputs = NoteInputs::new(vec![target.into()])?;
    let tag = NoteTag::from_account_id(target, NoteExecutionHint::Local)?;

    let metadata = NoteMetadata::new(sender, note_type, tag, aux)?;
    let vault = NoteAssets::new(assets)?;
    let recipient = NoteRecipient::new(serial_num, note_script, inputs);
    Ok(Note::new(vault, metadata, recipient))
}
