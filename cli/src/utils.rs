use core::panic;
use miden_client::{
    accounts::{AccountData, AccountId},
    assets::{Asset, FungibleAsset},
    auth::{StoreAuthenticator, TransactionAuthenticator},
    config::{Endpoint, RpcConfig},
    crypto::{FeltRng, RpoRandomCoin},
    notes::{NoteTag, NoteType},
    rpc::{NodeRpcClient, TonicRpcClient},
    store::{
        sqlite_store::{config::SqliteStoreConfig, SqliteStore},
        InputNoteRecord, NoteFilter, Store,
    },
    transactions::build_swap_tag,
    transactions::{
        request::{TransactionRequest, TransactionRequestError},
        OutputNote,
    },
    Client, Felt,
};
use miden_lib::utils::{Deserializable, Serializable};
use rand::{seq::SliceRandom, Rng};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
    rc::Rc,
};

use miden_lib::transaction::TransactionKernel;
use miden_objects::assembly::Assembler;
use miden_objects::Hasher;
use miden_objects::{
    notes::{
        Note, NoteAssets, NoteExecutionHint, NoteExecutionMode, NoteInputs, NoteMetadata,
        NoteRecipient, NoteScript,
    },
    NoteError, Word,
};

use crate::order::Order;

// Partially Fillable SWAP note
// ================================================================================================

/// Generates a SWAP note - swap of assets between two accounts - and returns the note as well as
/// [NoteDetails] for the payback note.
///
/// This script enables a swap of 2 assets between the `sender` account and any other account that
/// is willing to consume the note. The consumer will receive the `offered_asset` and will create a
/// new P2ID note with `sender` as target, containing the `requested_asset`.
///
/// # Errors
/// Returns an error if deserialization or compilation of the `SWAP` script fails.
pub fn create_partial_swap_note(
    creator: AccountId,
    last_consumer: AccountId,
    offered_asset: Asset,
    requested_asset: Asset,
    swap_serial_num: [Felt; 4],
    fill_number: u64,
) -> Result<Note, NoteError> {
    let assembler: Assembler = TransactionKernel::assembler_testing();

    let note_code = include_str!("../../swap_note/src/notes/PUBLIC_SWAPp.masm");
    let note_script = NoteScript::compile(note_code, assembler).unwrap();
    let note_type = NoteType::Public;

    let requested_asset_word: Word = requested_asset.into();
    let tag = build_swap_tag(
        note_type,
        offered_asset.faucet_id(),
        requested_asset.faucet_id(),
    )?;

    let inputs = NoteInputs::new(vec![
        requested_asset_word[0],
        requested_asset_word[1],
        requested_asset_word[2],
        requested_asset_word[3],
        tag.inner().into(),
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
        Felt::new(fill_number),
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
        creator.into(),
    ])?;

    let aux = Felt::new(0);

    // build the outgoing note
    let metadata = NoteMetadata::new(
        last_consumer,
        note_type,
        tag,
        NoteExecutionHint::always(),
        aux,
    )?;

    let assets = NoteAssets::new(vec![offered_asset])?;
    let recipient = NoteRecipient::new(swap_serial_num, note_script.clone(), inputs.clone());
    let note = Note::new(assets.clone(), metadata, recipient.clone());

    Ok(note)
}

pub fn create_p2id_note(
    sender: AccountId,
    target: AccountId,
    assets: Vec<Asset>,
    note_type: NoteType,
    aux: Felt,
    serial_num: [Felt; 4],
) -> Result<Note, NoteError> {
    let assembler: Assembler = TransactionKernel::assembler_testing().with_debug_mode(true);
    let note_code = include_str!("../../swap_note/src/notes/P2ID.masm");

    let note_script = NoteScript::compile(note_code, assembler).unwrap();

    let inputs = NoteInputs::new(vec![target.into()])?;
    let tag = NoteTag::from_account_id(target, NoteExecutionMode::Local)?;

    let metadata = NoteMetadata::new(sender, note_type, tag, NoteExecutionHint::always(), aux)?;
    let vault = NoteAssets::new(assets)?;
    let recipient = NoteRecipient::new(serial_num, note_script, inputs);
    Ok(Note::new(vault, metadata, recipient))
}

// Client Setup
// ================================================================================================

pub fn setup_client() -> Client<
    TonicRpcClient,
    RpoRandomCoin,
    SqliteStore,
    StoreAuthenticator<RpoRandomCoin, SqliteStore>,
> {
    let store_config = SqliteStoreConfig::default();
    let store = Rc::new(SqliteStore::new(&store_config).unwrap());
    let mut rng = rand::thread_rng();
    let coin_seed: [u64; 4] = rng.gen();
    let rng = RpoRandomCoin::new(coin_seed.map(Felt::new));
    let authenticator = StoreAuthenticator::new_with_rng(store.clone(), rng);
    let rpc_config = RpcConfig {
        endpoint: Endpoint::new("http".to_string(), "localhost".to_string(), 57291),
        timeout_ms: 10000,
    };
    let in_debug_mode = true;
    Client::new(
        TonicRpcClient::new(&rpc_config),
        rng,
        store,
        authenticator,
        in_debug_mode,
    )
}

// Transaction Request Creation
// ================================================================================================

pub fn create_swap_notes_transaction_request(
    num_notes: u8,
    sender: AccountId,
    offering_faucet: AccountId,
    total_asset_offering: u64,
    requesting_faucet: AccountId,
    total_asset_requesting: u64,
    felt_rng: &mut impl FeltRng,
) -> Result<TransactionRequest, TransactionRequestError> {
    // Setup note variables
    let mut own_output_notes = vec![];

    // Generate random distributions for offering and requesting assets
    let offering_distribution =
        generate_random_distribution(num_notes as usize, total_asset_offering);
    let requesting_distribution =
        generate_random_distribution(num_notes as usize, total_asset_requesting);

    let mut total_offering = 0;
    let mut total_requesting = 0;

    for i in 0..num_notes {
        let offered_asset = Asset::Fungible(
            FungibleAsset::new(offering_faucet, offering_distribution[i as usize]).unwrap(),
        );
        let requested_asset = Asset::Fungible(
            FungibleAsset::new(requesting_faucet, requesting_distribution[i as usize]).unwrap(),
        );

        let swap_serial_num = felt_rng.draw_word();
        let created_note = create_partial_swap_note(
            sender,
            sender,
            offered_asset,
            requested_asset,
            swap_serial_num,
            0,
        )?;
        // expected_future_notes.push(payback_note_details);
        total_offering += offering_distribution[i as usize];
        total_requesting += requesting_distribution[i as usize];
        println!(
            "{} - Created note with assets:\noffering: {:?}\nreal offering: {}\nrequesting: {}\nreal requesting: {}\ninputs: {:?}\ntag: {:?}\n",
            i,
            created_note.assets().iter().collect::<Vec<&Asset>>()[0].unwrap_fungible().amount(),
            offering_distribution[i as usize],
            created_note.inputs().values()[4],
            requesting_distribution[i as usize],
            created_note.inputs().values(),
            created_note.metadata().tag(),
        );
        println!("UserID: {:?}", sender.to_hex());
        own_output_notes.push(OutputNote::Full(created_note));
    }

    println!("Total generated offering asset: {}", total_offering);
    println!("Total generated requesting asset: {}", total_requesting);

    TransactionRequest::new().with_own_output_notes(own_output_notes)
}

//
pub fn generate_random_distribution(n: usize, total: u64) -> Vec<u64> {
    let min_value = 10;
    let max_value = 20;

    let total_min = n as u64 * min_value;
    let total_max = n as u64 * max_value;

    if total < total_min || total > total_max {
        panic!(
            "Total must be between {} and {} for {} numbers between {} and {}",
            total_min, total_max, n, min_value, max_value
        );
    }

    let mut result = vec![min_value; n]; // Start with the minimum value for all elements
    let mut total_remaining = total - total_min; // Remaining total to distribute

    let mut rng = rand::thread_rng();

    while total_remaining > 0 {
        for i in 0..n {
            if total_remaining == 0 {
                break;
            }

            // Calculate the maximum increment possible for the current element
            let max_increment = max_value - result[i];
            if max_increment == 0 {
                continue; // Skip if the current element has reached the max_value
            }

            // Generate a random increment between 1 and the lesser of max_increment and total_remaining
            let increment = rng.gen_range(1..=std::cmp::min(max_increment, total_remaining));
            result[i] += increment;
            total_remaining -= increment;
        }
    }

    // Optionally shuffle the result to randomize the order
    result.shuffle(&mut rng);

    result
}

// AccountData I/O
// ================================================================================================

pub fn export_account_data(account_data: &AccountData, filename: &str) -> io::Result<()> {
    let serialized = account_data.to_bytes();
    fs::create_dir_all("accounts")?;
    let file_path = Path::new("accounts").join(format!("{}.mac", filename));
    let mut file = File::create(file_path)?;
    file.write_all(&serialized)?;
    Ok(())
}

pub fn import_account_data(file_path: &str) -> io::Result<AccountData> {
    let mut file = File::open(file_path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    AccountData::read_from_bytes(&buffer)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

pub fn load_accounts() -> io::Result<Vec<AccountData>> {
    let accounts_dir = Path::new("accounts");

    if !accounts_dir.exists() {
        return Ok(Vec::new());
    }

    let mut accounts = Vec::new();

    for entry in fs::read_dir(accounts_dir)? {
        let entry = entry?;
        let path = entry.path();
        let path_str = path.to_str().unwrap();

        match import_account_data(path_str) {
            Ok(account_data) => accounts.push(account_data),
            Err(e) => eprintln!("Error importing account data from {} : {}", path_str, e),
        }
    }

    Ok(accounts)
}

pub fn sort_orders(mut orders: Vec<Order>) -> Vec<Order> {
    orders.sort_by(|a, b| {
        let a_price = a.price();
        let b_price = b.price();

        a_price
            .partial_cmp(&b_price)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    orders
}

pub fn get_notes_by_tag<N: NodeRpcClient, R: FeltRng, S: Store, A: TransactionAuthenticator>(
    client: &Client<N, R, S, A>,
    tag: NoteTag,
) -> Vec<InputNoteRecord> {
    let notes = client.get_input_notes(NoteFilter::All).unwrap();

    notes
        .into_iter()
        .filter(|note| note.metadata().unwrap().tag() == tag)
        .collect()
}

pub fn get_assets_from_swap_note(note: &InputNoteRecord) -> (Asset, Asset) {
    let source_asset =
        Asset::Fungible(note.assets().iter().collect::<Vec<&Asset>>()[0].unwrap_fungible());
    let target_faucet = AccountId::try_from(note.details().inputs()[3]).unwrap();
    let target_amount = note.details().inputs()[0].as_int();
    let target_asset = Asset::Fungible(FungibleAsset::new(target_faucet, target_amount).unwrap());
    (source_asset, target_asset)
}

pub fn print_order_table(orders: &[Order]) {
    let mut table = Vec::new();
    table.push("+--------------------------------------------------------------------+--------------------+------------------+--------------------+------------------+----------+".to_string());
    table.push("| Note ID                                                            | Requested Asset    | Amount Requested | Offered Asset      | Offered Amount   | Price    |".to_string());
    table.push("+--------------------------------------------------------------------+--------------------+------------------+--------------------+------------------+----------+".to_string());

    for order in orders {
        let note_id = order
            .id()
            .map_or_else(|| "N/A".to_string(), |id| id.to_string());
        let source_asset_faucet_id = order.source_asset().faucet_id().to_string();
        let source_asset_amount = order.source_asset().unwrap_fungible().amount();
        let target_asset_faucet_id = order.target_asset().faucet_id().to_string();
        let target_asset_amount = order.target_asset().unwrap_fungible().amount();

        table.push(format!(
            "| {:<66} | {:<16} | {:<16} | {:<16} | {:<16} | {:<8.2} |",
            note_id,
            target_asset_faucet_id,
            target_asset_amount,
            source_asset_faucet_id,
            source_asset_amount,
            order.price()
        ));
    }

    table.push("+--------------------------------------------------------------------+--------------------+------------------+--------------------+------------------+----------+".to_string());

    // Print table
    for line in table {
        println!("{}", line);
    }
}

pub fn compute_p2id_serial_num(swap_serial_num: [Felt; 4], swap_count: u64) -> [Felt; 4] {
    let swap_count_word = [
        Felt::new(swap_count),
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
    ];
    let p2id_serial_num = Hasher::merge(&[swap_serial_num.into(), swap_count_word.into()]);

    p2id_serial_num.into()
}
