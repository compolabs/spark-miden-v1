use miden_client::{
    errors::ClientError,
    rpc::NodeRpcClient,
    store::Store,
    transactions::transaction_request::{SwapTransactionData, TransactionTemplate},
    AccountTemplate, Client,
};
use miden_lib::{transaction::TransactionKernel, AuthScheme};
use miden_objects::{
    accounts::{
        auth, Account, AccountCode, AccountId, AccountStorage, AccountStorageType, AccountType,
        SlotItem,
    },
    assembly::ModuleAst,
    assets::{Asset, FungibleAsset, TokenSymbol},
    crypto::{dsa::rpo_falcon512::SecretKey, rand::FeltRng},
    notes::{NoteExecutionHint, NoteTag, NoteType},
    AccountError, Word,
};
use rand_chacha::{rand_core::SeedableRng, ChaCha20Rng};
use std::collections::BTreeMap;

use miden_tx::auth::TransactionAuthenticator;

use super::common::*;

// Define the wrapper enum
pub enum ExtendedAccountTemplate {
    BasicWallet {
        mutable_code: bool,
        storage_type: AccountStorageType,
    },
    FungibleFaucet {
        token_symbol: TokenSymbol,
        decimals: u8,
        max_supply: u64,
        storage_type: AccountStorageType,
    },
    CustomCodeWallet {
        init_seed: [u8; 32],
        auth_scheme: AuthScheme,
        account_storage_type: AccountStorageType,
        custom_code: String,
    },
}

// Implement conversion from ExtendedAccountTemplate to AccountTemplate
impl From<ExtendedAccountTemplate> for AccountTemplate {
    fn from(ext_template: ExtendedAccountTemplate) -> Self {
        match ext_template {
            ExtendedAccountTemplate::BasicWallet {
                mutable_code,
                storage_type,
            } => AccountTemplate::BasicWallet {
                mutable_code,
                storage_type,
            },
            ExtendedAccountTemplate::FungibleFaucet {
                token_symbol,
                decimals,
                max_supply,
                storage_type,
            } => AccountTemplate::FungibleFaucet {
                token_symbol,
                decimals,
                max_supply,
                storage_type,
            },
            _ => panic!("Unsupported template variant for conversion"), // Handle CustomCodeWallet separately
        }
    }
}

// Define the CustomClient wrapper struct
pub struct CustomClient<N: NodeRpcClient, R: FeltRng, S: Store, A: TransactionAuthenticator> {
    client: Client<N, R, S, A>,
}

impl<N: NodeRpcClient, R: FeltRng, S: Store, A: TransactionAuthenticator> CustomClient<N, R, S, A> {
    pub fn new(client: Client<N, R, S, A>) -> Self {
        Self { client }
    }

    pub fn create_custom_code_wallet(
        &mut self,
        init_seed: [u8; 32],
        auth_scheme: AuthScheme,
        account_storage_type: AccountStorageType,
        custom_code: String,
    ) -> Result<(Account, Word), ClientError> {
        let account_type = AccountType::RegularAccountImmutableCode; // Adjust this as per your enum definition

        let (auth_scheme_procedure, storage_slot_0_data): (&str, Word) = match auth_scheme {
            AuthScheme::RpoFalcon512 { pub_key } => {
                ("basic::auth_tx_rpo_falcon512", pub_key.into())
            }
        };

        let account_code_string: String = format!(
            "
            {custom_code}

            export.{auth_scheme_procedure}
        "
        );
        let account_code_src: &str = &account_code_string;

        let account_code_ast = ModuleAst::parse(account_code_src)
            .map_err(|e| AccountError::AccountCodeAssemblerError(e.into()))?;
        let account_assembler = TransactionKernel::assembler();
        let account_code = AccountCode::new(account_code_ast.clone(), &account_assembler)?;

        let account_storage = AccountStorage::new(
            vec![SlotItem::new_value(0, 0, storage_slot_0_data)],
            BTreeMap::new(),
        )?;

        let account_seed = AccountId::get_account_seed(
            init_seed,
            account_type,
            account_storage_type,
            account_code.root(),
            account_storage.root(),
        )?;

        Ok((
            Account::new(account_seed, account_code, account_storage)?,
            account_seed,
        ))
    }

    pub fn new_account(
        &mut self,
        template: ExtendedAccountTemplate,
    ) -> Result<(Account, Word), ClientError> {
        match template {
            ExtendedAccountTemplate::CustomCodeWallet {
                init_seed,
                auth_scheme,
                account_storage_type,
                custom_code,
            } => self.create_custom_code_wallet(
                init_seed,
                auth_scheme,
                account_storage_type,
                custom_code,
            ),
            other => self.client.new_account(other.into()),
        }
    }
}

// Implementing the extension trait as before.
pub trait AccountTemplateExt {
    fn custom_code_wallet(
        init_seed: [u8; 32],
        auth_scheme: AuthScheme,
        account_storage_type: AccountStorageType,
        custom_code: String,
    ) -> ExtendedAccountTemplate;
}

impl AccountTemplateExt for ExtendedAccountTemplate {
    fn custom_code_wallet(
        init_seed: [u8; 32],
        auth_scheme: AuthScheme,
        account_storage_type: AccountStorageType,
        custom_code: String,
    ) -> ExtendedAccountTemplate {
        ExtendedAccountTemplate::CustomCodeWallet {
            init_seed,
            auth_scheme,
            account_storage_type,
            custom_code,
        }
    }
}

#[tokio::test]
async fn test_deploy_custom_account_wallet() {
    const OFFERED_ASSET_AMOUNT: u64 = 1;
    const REQUESTED_ASSET_AMOUNT: u64 = 25;
    const BTC_MINT_AMOUNT: u64 = 1000;
    const ETH_MINT_AMOUNT: u64 = 1000;
    let mut client1 = create_test_client();
    let mut custom_client1 = CustomClient::new(client1);

    wait_for_node(&mut custom_client1.client).await;
    let mut client2 = create_test_client();
    let mut custom_client2 = CustomClient::new(client2);
    let mut client_with_faucets = create_test_client();
    let mut custom_client_with_faucets = CustomClient::new(client_with_faucets);

    custom_client1.client.sync_state().await.unwrap();
    custom_client2.client.sync_state().await.unwrap();
    custom_client_with_faucets
        .client
        .sync_state()
        .await
        .unwrap();

    // Create Client 1's custom code wallet (We'll call it accountA)
    let custom_code = "
    use.miden::contracts::auth::basic
    use.miden::contracts::wallets::basic->basic_wallet
    use.miden::tx
    use.miden::account
    
    export.basic_wallet::receive_asset
    export.basic_wallet::send_asset
    
    # get token balance
    export.account::get_balance
    
    ### Notice ####
    # The following procedures need to be hidden
    
    # create note
    export.tx::create_note
    
    # add asset to note
    export.tx::add_asset_to_note
    
    # remove asset from account
    export.account::remove_asset
    
    # increment counter
    export.account::incr_nonce
    
    # SWAPP OWNER REQUIRED PROC
    # get account id
    export.account::get_id
    "
    .to_string();
    let init_seed = [
        95, 113, 209, 94, 84, 105, 250, 242, 223, 203, 216, 124, 22, 159, 14, 132, 215, 85, 183,
        204, 149, 90, 166, 68, 100, 73, 106, 168, 125, 237, 138, 16,
    ];
    let seed = [0_u8; 32];
    let mut rng = ChaCha20Rng::from_seed(seed);

    let sec_key = SecretKey::with_rng(&mut rng);
    let pub_key = sec_key.public_key();
    let auth_scheme: AuthScheme = AuthScheme::RpoFalcon512 { pub_key };

    let account_storage_type = AccountStorageType::OffChain;

    let (account_a, _) = custom_client1
        .new_account(ExtendedAccountTemplate::CustomCodeWallet {
            init_seed,
            auth_scheme,
            account_storage_type,
            custom_code,
        })
        .unwrap();

    println!("account A: {:?}", account_a);
}

// SWAP FULLY ONCHAIN
// ================================================================================================

#[tokio::test]
async fn test_swap_fully_onchain() {
    const OFFERED_ASSET_AMOUNT: u64 = 1;
    const REQUESTED_ASSET_AMOUNT: u64 = 25;
    const BTC_MINT_AMOUNT: u64 = 1000;
    const ETH_MINT_AMOUNT: u64 = 1000;

    // Create clients and custom clients
    let mut client1 = create_test_client();
    let mut custom_client1 = CustomClient::new(client1);
    let mut client2 = create_test_client();
    let mut custom_client2 = CustomClient::new(client2);
    let mut client_with_faucets = create_test_client();
    let mut custom_client_with_faucets = CustomClient::new(client_with_faucets);

    // Sync clients
    custom_client1.client.sync_state().await.unwrap();
    custom_client2.client.sync_state().await.unwrap();
    custom_client_with_faucets
        .client
        .sync_state()
        .await
        .unwrap();

    // Create Client 1's custom code wallet (account A)
    let custom_code = "
    use.miden::contracts::auth::basic
    use.miden::contracts::wallets::basic->basic_wallet
    use.miden::tx
    use.miden::account
    
    export.basic_wallet::receive_asset
    export.basic_wallet::send_asset
    
    # get token balance
    export.account::get_balance
    
    ### Notice ####
    # The following procedures need to be hidden
    
    # create note
    export.tx::create_note
    
    # add asset to note
    export.tx::add_asset_to_note
    
    # remove asset from account
    export.account::remove_asset
    
    # increment counter
    export.account::incr_nonce
    
    # SWAPP OWNER REQUIRED PROC
    # get account id
    export.account::get_id
    "
    .to_string();
    let init_seed = [
        95, 113, 209, 94, 84, 105, 250, 242, 223, 203, 216, 124, 22, 159, 14, 132, 215, 85, 183,
        204, 149, 90, 166, 68, 100, 73, 106, 168, 125, 237, 138, 16,
    ];
    let seed = [0_u8; 32];
    let mut rng = ChaCha20Rng::from_seed(seed);
    let sec_key = SecretKey::with_rng(&mut rng);
    let pub_key = sec_key.public_key();
    let auth_scheme: AuthScheme = AuthScheme::RpoFalcon512 {
        pub_key: pub_key.clone(),
    };
    let account_storage_type = AccountStorageType::OffChain;

    let (account_a, _) = custom_client1
        .new_account(ExtendedAccountTemplate::CustomCodeWallet {
            init_seed,
            auth_scheme: AuthScheme::RpoFalcon512 {
                pub_key: pub_key.clone(),
            },
            account_storage_type,
            custom_code: custom_code.clone(),
        })
        .unwrap();

    // Create Client 2's custom code wallet (account B)
    let (account_b, _) = custom_client2
        .new_account(ExtendedAccountTemplate::CustomCodeWallet {
            init_seed,
            auth_scheme: AuthScheme::RpoFalcon512 { pub_key },
            account_storage_type,
            custom_code,
        })
        .unwrap();

    // Create client with faucets BTC faucet (note: it's not real BTC)
    let (btc_faucet_account, _) = custom_client_with_faucets
        .client
        .new_account(AccountTemplate::FungibleFaucet {
            token_symbol: TokenSymbol::new("BTC").unwrap(),
            decimals: 8,
            max_supply: 1_000_000,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();

    // Create client with faucets ETH faucet (note: it's not real ETH)
    let (eth_faucet_account, _) = custom_client_with_faucets
        .client
        .new_account(AccountTemplate::FungibleFaucet {
            token_symbol: TokenSymbol::new("ETH").unwrap(),
            decimals: 8,
            max_supply: 1_000_000,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();

    // Mint 1000 BTC for account A
    println!("minting 1000 btc for account A");
    mint(
        &mut custom_client_with_faucets.client,
        account_a.id(),
        btc_faucet_account.id(),
        NoteType::Public,
        BTC_MINT_AMOUNT,
    )
    .await;

    // Mint 1000 ETH for account B
    println!("minting 1000 eth for account B");
    mint(
        &mut custom_client_with_faucets.client,
        account_b.id(),
        eth_faucet_account.id(),
        NoteType::Public,
        ETH_MINT_AMOUNT,
    )
    .await;

    // Sync and consume note for account A
    custom_client1.client.sync_state().await.unwrap();
    let client_1_notes = custom_client1
        .client
        .get_input_notes(miden_client::store::NoteFilter::All)
        .unwrap();
    assert_eq!(client_1_notes.len(), 1);

    println!("Consuming mint note on first client...");
    let tx_template =
        TransactionTemplate::ConsumeNotes(account_a.id(), vec![client_1_notes[0].id()]);
    let tx_request = custom_client1
        .client
        .build_transaction_request(tx_template)
        .unwrap();
    execute_tx_and_sync(&mut custom_client1.client, tx_request).await;

    // Sync and consume note for account B
    custom_client2.client.sync_state().await.unwrap();
    let client_2_notes = custom_client2
        .client
        .get_input_notes(miden_client::store::NoteFilter::All)
        .unwrap();
    assert_eq!(client_2_notes.len(), 1);

    println!("Consuming mint note on second client...");
    let tx_template =
        TransactionTemplate::ConsumeNotes(account_b.id(), vec![client_2_notes[0].id()]);
    let tx_request = custom_client2
        .client
        .build_transaction_request(tx_template)
        .unwrap();
    execute_tx_and_sync(&mut custom_client2.client, tx_request).await;

    // @dev The Miden Client needs to be updated to allow for custom notes to be created and ideally custom wallets
    /*
       // Create ONCHAIN swap note (client A offers 1 BTC in exchange for 25 ETH)
       println!("creating swap note with account A");
       let offered_asset = FungibleAsset::new(btc_faucet_account.id(), OFFERED_ASSET_AMOUNT).unwrap();
       let requested_asset =
           FungibleAsset::new(eth_faucet_account.id(), REQUESTED_ASSET_AMOUNT).unwrap();
       let tx_template = TransactionTemplate::Swap(
           SwapTransactionData::new(
               account_a.id(),
               Asset::Fungible(offered_asset),
               Asset::Fungible(requested_asset),
           ),
           NoteType::Public,
       );

       println!("Running SWAP tx...");
       let tx_request = custom_client1.client.build_transaction_request(tx_template).unwrap();

       let expected_output_notes = tx_request.expected_output_notes().to_vec();
       let expected_payback_note_details = tx_request.expected_partial_notes().to_vec();
       assert_eq!(expected_output_notes.len(), 1);
       assert_eq!(expected_payback_note_details.len(), 1);

       execute_tx_and_sync(&mut custom_client1.client, tx_request).await;

       let payback_note_tag = build_swap_tag(
           NoteType::Public,
           btc_faucet_account.id(),
           eth_faucet_account.id(),
       );

       // Add swap note's tag to both clients
       println!("Adding swap tags");
       custom_client1.client.add_note_tag(payback_note_tag).unwrap();
       custom_client2.client.add_note_tag(payback_note_tag).unwrap();

       // Sync on client 2, consume swap note with account B
       custom_client2.client.sync_state().await.unwrap();
       println!("Consuming swap note on second client...");
       let tx_template =
           TransactionTemplate::ConsumeNotes(account_b.id(), vec![expected_output_notes[0].id()]);
       let tx_request = custom_client2.client.build_transaction_request(tx_template).unwrap();
       execute_tx_and_sync(&mut custom_client2.client, tx_request).await;

       // Sync on client 1, consume received note with account A
       custom_client1.client.sync_state().await.unwrap();
       println!("Consuming swap payback note on first client...");
       let tx_template = TransactionTemplate::ConsumeNotes(
           account_a.id(),
           vec![expected_payback_note_details[0].id()],
       );
       let tx_request = custom_client1.client.build_transaction_request(tx_template).unwrap();
       execute_tx_and_sync(&mut custom_client1.client, tx_request).await;
    */
}

#[tokio::test]
async fn test_swap_offchain() {
    const OFFERED_ASSET_AMOUNT: u64 = 1;
    const REQUESTED_ASSET_AMOUNT: u64 = 25;
    const BTC_MINT_AMOUNT: u64 = 1000;
    const ETH_MINT_AMOUNT: u64 = 1000;
    let mut client1 = create_test_client();
    wait_for_node(&mut client1).await;
    let mut client2 = create_test_client();
    let mut client_with_faucets = create_test_client();

    client1.sync_state().await.unwrap();
    client2.sync_state().await.unwrap();
    client_with_faucets.sync_state().await.unwrap();

    // Create Client 1's basic wallet (We'll call it accountA)
    let (account_a, _) = client1
        .new_account(AccountTemplate::BasicWallet {
            mutable_code: false,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();

    // Create Client 2's basic wallet (We'll call it accountB)
    let (account_b, _) = client2
        .new_account(AccountTemplate::BasicWallet {
            mutable_code: false,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();

    // Create client with faucets BTC faucet (note: it's not real BTC)
    let (btc_faucet_account, _) = client_with_faucets
        .new_account(AccountTemplate::FungibleFaucet {
            token_symbol: TokenSymbol::new("BTC").unwrap(),
            decimals: 8,
            max_supply: 1_000_000,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();
    // Create client with faucets ETH faucet (note: it's not real ETH)
    let (eth_faucet_account, _) = client_with_faucets
        .new_account(AccountTemplate::FungibleFaucet {
            token_symbol: TokenSymbol::new("ETH").unwrap(),
            decimals: 8,
            max_supply: 1_000_000,
            storage_type: AccountStorageType::OffChain,
        })
        .unwrap();

    // mint 1000 BTC for accountA
    println!("minting 1000 btc for account A");
    mint(
        &mut client_with_faucets,
        account_a.id(),
        btc_faucet_account.id(),
        NoteType::Public,
        BTC_MINT_AMOUNT,
    )
    .await;
    // mint 1000 ETH for accountB
    println!("minting 1000 eth for account B");
    mint(
        &mut client_with_faucets,
        account_b.id(),
        eth_faucet_account.id(),
        NoteType::Public,
        ETH_MINT_AMOUNT,
    )
    .await;

    // Sync and consume note for accountA
    client1.sync_state().await.unwrap();
    let client_1_notes = client1
        .get_input_notes(miden_client::store::NoteFilter::All)
        .unwrap();
    assert_eq!(client_1_notes.len(), 1);

    println!("Consuming mint note on first client...");
    let tx_template =
        TransactionTemplate::ConsumeNotes(account_a.id(), vec![client_1_notes[0].id()]);
    let tx_request = client1.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(&mut client1, tx_request).await;

    // Sync and consume note for accountB
    client2.sync_state().await.unwrap();
    let client_2_notes = client2
        .get_input_notes(miden_client::store::NoteFilter::All)
        .unwrap();
    assert_eq!(client_2_notes.len(), 1);

    println!("Consuming mint note on second client...");
    let tx_template =
        TransactionTemplate::ConsumeNotes(account_b.id(), vec![client_2_notes[0].id()]);
    let tx_request = client2.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(&mut client2, tx_request).await;

    // Create ONCHAIN swap note (clientA offers 1 BTC in exchange of 25 ETH)
    // check that account now has 1 less BTC
    println!("creating swap note with accountA");
    let offered_asset = FungibleAsset::new(btc_faucet_account.id(), OFFERED_ASSET_AMOUNT).unwrap();
    let requested_asset =
        FungibleAsset::new(eth_faucet_account.id(), REQUESTED_ASSET_AMOUNT).unwrap();
    let tx_template = TransactionTemplate::Swap(
        SwapTransactionData::new(
            account_a.id(),
            Asset::Fungible(offered_asset),
            Asset::Fungible(requested_asset),
        ),
        NoteType::OffChain,
    );
    println!("Running SWAP tx...");
    let tx_request = client1.build_transaction_request(tx_template).unwrap();

    let expected_output_notes = tx_request.expected_output_notes().to_vec();
    let expected_payback_note_details = tx_request.expected_partial_notes().to_vec();
    assert_eq!(expected_output_notes.len(), 1);
    assert_eq!(expected_payback_note_details.len(), 1);

    execute_tx_and_sync(&mut client1, tx_request).await;

    // Export note from client 1 to client 2
    let exported_note = client1
        .get_output_note(expected_output_notes[0].id())
        .unwrap();

    client2
        .import_input_note(exported_note.try_into().unwrap(), true)
        .await
        .unwrap();

    // Sync so we get the inclusion proof info
    client2.sync_state().await.unwrap();

    // consume swap note with accountB, and check that the vault changed appropiately
    println!("Consuming swap note on second client...");
    let tx_template =
        TransactionTemplate::ConsumeNotes(account_b.id(), vec![expected_output_notes[0].id()]);
    let tx_request = client2.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(&mut client2, tx_request).await;

    // sync on client 1, we should get the missing payback note details.
    // try consuming the received note with accountA, it should now have 25 ETH
    client1.sync_state().await.unwrap();
    println!("Consuming swap payback note on first client...");
    let tx_template = TransactionTemplate::ConsumeNotes(
        account_a.id(),
        vec![expected_payback_note_details[0].id()],
    );
    let tx_request = client1.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(&mut client1, tx_request).await;

    // At the end we should end up with
    //
    // - accountA: 999 BTC, 25 ETH
    // - accountB: 1 BTC, 975 ETH

    // first reload the account
    let (account_a, _) = client1.get_account(account_a.id()).unwrap();
    let account_a_assets = account_a.vault().assets();
    assert_eq!(account_a_assets.count(), 2);
    let mut account_a_assets = account_a.vault().assets();

    let asset_1 = account_a_assets.next().unwrap();
    let asset_2 = account_a_assets.next().unwrap();

    match (asset_1, asset_2) {
        (Asset::Fungible(btc_asset), Asset::Fungible(eth_asset))
            if btc_asset.faucet_id() == btc_faucet_account.id()
                && eth_asset.faucet_id() == eth_faucet_account.id() =>
        {
            assert_eq!(btc_asset.amount(), 999);
            assert_eq!(eth_asset.amount(), 25);
        }
        (Asset::Fungible(eth_asset), Asset::Fungible(btc_asset))
            if btc_asset.faucet_id() == btc_faucet_account.id()
                && eth_asset.faucet_id() == eth_faucet_account.id() =>
        {
            assert_eq!(btc_asset.amount(), 999);
            assert_eq!(eth_asset.amount(), 25);
        }
        _ => panic!("should only have fungible assets!"),
    }

    let (account_b, _) = client2.get_account(account_b.id()).unwrap();
    let account_b_assets = account_b.vault().assets();
    assert_eq!(account_b_assets.count(), 2);
    let mut account_b_assets = account_b.vault().assets();

    let asset_1 = account_b_assets.next().unwrap();
    let asset_2 = account_b_assets.next().unwrap();

    match (asset_1, asset_2) {
        (Asset::Fungible(btc_asset), Asset::Fungible(eth_asset))
            if btc_asset.faucet_id() == btc_faucet_account.id()
                && eth_asset.faucet_id() == eth_faucet_account.id() =>
        {
            assert_eq!(btc_asset.amount(), 1);
            assert_eq!(eth_asset.amount(), 975);
        }
        (Asset::Fungible(eth_asset), Asset::Fungible(btc_asset))
            if btc_asset.faucet_id() == btc_faucet_account.id()
                && eth_asset.faucet_id() == eth_faucet_account.id() =>
        {
            assert_eq!(btc_asset.amount(), 1);
            assert_eq!(eth_asset.amount(), 975);
        }
        _ => panic!("should only have fungible assets!"),
    }
}

/// Returns a note tag for a swap note with the specified parameters.
///
/// Use case ID for the returned tag is set to 0.
///
/// Tag payload is constructed by taking asset tags (8 bits of faucet ID) and concatenating them
/// together as offered_asset_tag + requested_asset tag.
///
/// Network execution hint for the returned tag is set to `Local`.
///
/// Based on miden-base's implementation (<https://github.com/0xPolygonMiden/miden-base/blob/9e4de88031b55bcc3524cb0ccfb269821d97fb29/miden-lib/src/notes/mod.rs#L153>)
fn build_swap_tag(
    note_type: NoteType,
    offered_asset_faucet_id: AccountId,
    requested_asset_faucet_id: AccountId,
) -> NoteTag {
    const SWAP_USE_CASE_ID: u16 = 0;

    // get bits 4..12 from faucet IDs of both assets, these bits will form the tag payload; the
    // reason we skip the 4 most significant bits is that these encode metadata of underlying
    // faucets and are likely to be the same for many different faucets.

    let offered_asset_id: u64 = offered_asset_faucet_id.into();
    let offered_asset_tag = (offered_asset_id >> 52) as u8;

    let requested_asset_id: u64 = requested_asset_faucet_id.into();
    let requested_asset_tag = (requested_asset_id >> 52) as u8;

    let payload = ((offered_asset_tag as u16) << 8) | (requested_asset_tag as u16);

    let execution = NoteExecutionHint::Local;
    match note_type {
        NoteType::Public => NoteTag::for_public_use_case(SWAP_USE_CASE_ID, payload, execution),
        _ => NoteTag::for_local_use_case(SWAP_USE_CASE_ID, payload),
    }
    .unwrap()
}

/// Mints a note from faucet_account_id for basic_account_id, waits for inclusion and returns it
/// with 1000 units of the corresponding fungible asset
///
/// `basic_account_id` does not need to be tracked by the client, but `faucet_account_id` does
async fn mint(
    client: &mut TestClient,
    basic_account_id: AccountId,
    faucet_account_id: AccountId,
    note_type: NoteType,
    mint_amount: u64,
) {
    // Create a Mint Tx for 1000 units of our fungible asset
    let fungible_asset = FungibleAsset::new(faucet_account_id, mint_amount).unwrap();
    let tx_template =
        TransactionTemplate::MintFungibleAsset(fungible_asset, basic_account_id, note_type);

    println!("Minting Asset");
    let tx_request = client.build_transaction_request(tx_template).unwrap();
    execute_tx_and_sync(client, tx_request.clone()).await;
}
