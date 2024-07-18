use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{Account, AccountCode, AccountId, AccountStorage, SlotItem},
    assembly::ModuleAst,
    assets::{Asset, AssetVault},
    crypto::{dsa::rpo_falcon512::SecretKey, utils::Serializable},
    transaction::{ExecutedTransaction, ProvenTransaction},
    Felt, Word,
};
use miden_processor::utils::Deserializable;
use miden_prover::ProvingOptions;
use miden_tx::{TransactionProver, TransactionVerifier, TransactionVerifierError};
use rand_chacha::{rand_core::SeedableRng, ChaCha20Rng};

// pub const ACCOUNT_ID_REGULAR_ACCOUNT_UPDATABLE_CODE_OFF_CHAIN: u64 = 0x900000000000003F; // 10376293541461622847
pub const ACCOUNT_ID_SENDER: u64 = 0x800000000000001F; // 9223372036854775839
pub const ACCOUNT_ID_SENDER_1: u64 = 0x800000000000002F; // 9223372036854775840
pub const ACCOUNT_ID_SENDER_2: u64 = 0x800000000000003F; // 9223372036854775841

// ACCOUNT TYPES
// ================================================================================================

pub const FUNGIBLE_FAUCET: u64 = 0b10;
pub const NON_FUNGIBLE_FAUCET: u64 = 0b11;
pub const REGULAR_ACCOUNT_IMMUTABLE_CODE: u64 = 0b00;
pub const REGULAR_ACCOUNT_UPDATABLE_CODE: u64 = 0b01;

// ACCOUNT STORAGE TYPES
// ================================================================================================

// CONSTANTS
// ================================================================================================

// The higher two bits of the most significant nibble determines the account type
pub const ACCOUNT_STORAGE_MASK_SHIFT: u64 = 62;
// pub const ACCOUNT_STORAGE_MASK: u64 = 0b11 << ACCOUNT_STORAGE_MASK_SHIFT;

// The lower two bits of the most significant nibble determines the account type
pub const ACCOUNT_TYPE_MASK_SHIFT: u64 = 60;
pub const ACCOUNT_TYPE_MASK: u64 = 0b11 << ACCOUNT_TYPE_MASK_SHIFT;
// pub const ACCOUNT_ISFAUCET_MASK: u64 = 0b10 << ACCOUNT_TYPE_MASK_SHIFT;

pub const ON_CHAIN: u64 = 0b00;
// pub const OFF_CHAIN: u64 = 0b10;

// UTILITIES
// --------------------------------------------------------------------------------------------

#[repr(u64)]
pub enum AccountType {
    FungibleFaucet = FUNGIBLE_FAUCET,
    NonFungibleFaucet = NON_FUNGIBLE_FAUCET,
    RegularAccountImmutableCode = REGULAR_ACCOUNT_IMMUTABLE_CODE,
    RegularAccountUpdatableCode = REGULAR_ACCOUNT_UPDATABLE_CODE,
}

/// Returns the [AccountType] given an integer representation of `account_id`.
impl From<u64> for AccountType {
    fn from(value: u64) -> Self {
        debug_assert!(
            ACCOUNT_TYPE_MASK.count_ones() == 2,
            "This method assumes there are only 2bits in the mask"
        );

        let bits = (value & ACCOUNT_TYPE_MASK) >> ACCOUNT_TYPE_MASK_SHIFT;
        match bits {
            REGULAR_ACCOUNT_UPDATABLE_CODE => AccountType::RegularAccountUpdatableCode,
            REGULAR_ACCOUNT_IMMUTABLE_CODE => AccountType::RegularAccountImmutableCode,
            FUNGIBLE_FAUCET => AccountType::FungibleFaucet,
            NON_FUNGIBLE_FAUCET => AccountType::NonFungibleFaucet,
            _ => {
                unreachable!("account_type mask contains only 2bits, there are 4 options total")
            }
        }
    }
}

#[repr(u64)]
pub enum AccountStorageType {
    OnChain = ON_CHAIN,
    // OffChain = OFF_CHAIN,
}
pub const fn account_id(account_type: AccountType, storage: AccountStorageType, rest: u64) -> u64 {
    let mut id = 0;

    id ^= (storage as u64) << ACCOUNT_STORAGE_MASK_SHIFT;
    id ^= (account_type as u64) << ACCOUNT_TYPE_MASK_SHIFT;
    id ^= rest;

    id
}
// CONSTANTS
// --------------------------------------------------------------------------------------------

/* pub const ACCOUNT_ID_OFF_CHAIN_SENDER: u64 = account_id(
    AccountType::RegularAccountImmutableCode,
    AccountStorageType::OffChain,
    0b0010_1111,
); */
// REGULAR ACCOUNTS - ON-CHAIN
/* pub const ACCOUNT_ID_REGULAR_ACCOUNT_IMMUTABLE_CODE_ON_CHAIN: u64 = account_id(
    AccountType::RegularAccountImmutableCode,
    AccountStorageType::OnChain,
    0b0001_1111,
); */
/* pub const ACCOUNT_ID_REGULAR_ACCOUNT_IMMUTABLE_CODE_ON_CHAIN_2: u64 = account_id(
    AccountType::RegularAccountImmutableCode,
    AccountStorageType::OnChain,
    0b0010_1111,
);
pub const ACCOUNT_ID_REGULAR_ACCOUNT_UPDATABLE_CODE_ON_CHAIN: u64 = account_id(
    AccountType::RegularAccountUpdatableCode,
    AccountStorageType::OnChain,
    0b0011_1111,
); */
/* pub const ACCOUNT_ID_REGULAR_ACCOUNT_UPDATABLE_CODE_ON_CHAIN_2: u64 = account_id(
    AccountType::RegularAccountUpdatableCode,
    AccountStorageType::OnChain,
    0b0100_1111,
); */

// FUNGIBLE TOKENS - OFF-CHAIN
/* pub const ACCOUNT_ID_FUNGIBLE_FAUCET_OFF_CHAIN: u64 = account_id(
    AccountType::FungibleFaucet,
    AccountStorageType::OffChain,
    0b0001_1111,
); */
// FUNGIBLE TOKENS - ON-CHAIN
pub const ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN: u64 = account_id(
    AccountType::FungibleFaucet,
    AccountStorageType::OnChain,
    0b0001_1111,
);
pub const ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1: u64 = account_id(
    AccountType::FungibleFaucet,
    AccountStorageType::OnChain,
    0b0010_1111,
);
/*
pub const ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_2: u64 = account_id(
    AccountType::FungibleFaucet,
    AccountStorageType::OnChain,
    0b0011_1111,
);
pub const ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_3: u64 = account_id(
    AccountType::FungibleFaucet,
    AccountStorageType::OnChain,
    0b0100_1111,
); */

// NON-FUNGIBLE TOKENS - OFF-CHAIN
/* pub const ACCOUNT_ID_INSUFFICIENT_ONES: u64 = account_id(
    AccountType::NonFungibleFaucet,
    AccountStorageType::OffChain,
    0b0000_0000,
); // invalid
pub const ACCOUNT_ID_NON_FUNGIBLE_FAUCET_OFF_CHAIN: u64 = account_id(
    AccountType::NonFungibleFaucet,
    AccountStorageType::OffChain,
    0b0001_1111,
); */
// NON-FUNGIBLE TOKENS - ON-CHAIN
/* pub const ACCOUNT_ID_NON_FUNGIBLE_FAUCET_ON_CHAIN: u64 = account_id(
    AccountType::NonFungibleFaucet,
    AccountStorageType::OnChain,
    0b0010_1111,
); */

/* pub const ACCOUNT_ID_NON_FUNGIBLE_FAUCET_ON_CHAIN_1: u64 = account_id(
    AccountType::NonFungibleFaucet,
    AccountStorageType::OnChain,
    0b0011_1111,
);
 */
// MOCK DATA STORE
// ================================================================================================

/* #[derive(Clone)]
pub struct MockDataStore {
    pub account: Account,
    pub block_header: BlockHeader,
    pub block_chain: ChainMmr,
    pub notes: Vec<InputNote>,
    pub tx_args: TransactionArgs,
}

impl MockDataStore {
    pub fn new() -> Self {
        let (tx_inputs, tx_args) = mock_inputs(
            MockAccountType::StandardExisting,
            AssetPreservationStatus::Preserved,
        );
        let (account, _, block_header, block_chain, notes) = tx_inputs.into_parts();
        Self {
            account,
            block_header,
            block_chain,
            notes: notes.into_vec(),
            tx_args,
        }
    }

    pub fn with_existing(account: Option<Account>, input_notes: Option<Vec<Note>>) -> Self {
        let (
            account,
            block_header,
            block_chain,
            consumed_notes,
            _auxiliary_data_inputs,
            created_notes,
        ) = mock_inputs_with_existing(
            MockAccountType::StandardExisting,
            AssetPreservationStatus::Preserved,
            account,
            input_notes,
        );
        let output_notes = created_notes.into_iter().filter_map(|note| match note {
            OutputNote::Full(note) => Some(note),
            OutputNote::Header(_) => None,
        });
        let mut tx_args = TransactionArgs::default();
        tx_args.extend_expected_output_notes(output_notes);

        Self {
            account,
            block_header,
            block_chain,
            notes: consumed_notes,
            tx_args,
        }
    }
}

impl Default for MockDataStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DataStore for MockDataStore {
    fn get_transaction_inputs(
        &self,
        account_id: AccountId,
        block_num: u32,
        notes: &[NoteId],
    ) -> Result<TransactionInputs, DataStoreError> {
        assert_eq!(account_id, self.account.id());
        assert_eq!(block_num, self.block_header.block_num());
        assert_eq!(notes.len(), self.notes.len());

        let notes = self
            .notes
            .iter()
            .filter(|note| notes.contains(&note.id()))
            .cloned()
            .collect::<Vec<_>>();

        Ok(TransactionInputs::new(
            self.account.clone(),
            None,
            self.block_header,
            self.block_chain.clone(),
            InputNotes::new(notes).unwrap(),
        )
        .unwrap())
    }

    fn get_account_code(&self, account_id: AccountId) -> Result<ModuleAst, DataStoreError> {
        assert_eq!(account_id, self.account.id());
        Ok(self.account.code().module().clone())
    }
}
 */
// HELPER FUNCTIONS
// ================================================================================================

#[cfg(test)]
pub fn prove_and_verify_transaction(
    executed_transaction: ExecutedTransaction,
) -> Result<(), TransactionVerifierError> {
    let executed_transaction_id = executed_transaction.id();
    // Prove the transaction

    let proof_options = ProvingOptions::default();
    let prover = TransactionProver::new(proof_options);
    let proven_transaction = prover.prove_transaction(executed_transaction).unwrap();

    assert_eq!(proven_transaction.id(), executed_transaction_id);

    // Serialize & deserialize the ProvenTransaction
    let serialised_transaction = proven_transaction.to_bytes();
    let proven_transaction = ProvenTransaction::read_from_bytes(&serialised_transaction).unwrap();

    // Verify that the generated proof is valid
    let verifier = TransactionVerifier::new(miden_objects::MIN_PROOF_SECURITY_LEVEL);

    verifier.verify(proven_transaction)
}

/* #[cfg(test)]
pub fn get_new_key_pair_with_advice_map() -> (Word, Vec<Felt>) {
    let seed = [0_u8; 32];
    let mut rng = ChaCha20Rng::from_seed(seed);

    let sec_key = SecretKey::with_rng(&mut rng);
    let pub_key: Word = sec_key.public_key().into();
    let mut pk_sk_bytes = sec_key.to_bytes();
    pk_sk_bytes.append(&mut pub_key.to_bytes());
    let pk_sk_felts: Vec<Felt> = pk_sk_bytes
        .iter()
        .map(|a| Felt::new(*a as u64))
        .collect::<Vec<Felt>>();

    (pub_key, pk_sk_felts)
} */

/* #[cfg(test)]
pub fn get_account_with_default_account_code(
    account_id: AccountId,
    public_key: Word,
    assets: Option<Asset>,
) -> Account {
    let account_code_src = DEFAULT_ACCOUNT_CODE;
    let account_code_ast = ModuleAst::parse(account_code_src).unwrap();
    let account_assembler = TransactionKernel::assembler().with_debug_mode(true);

    let account_code = AccountCode::new(account_code_ast.clone(), &account_assembler).unwrap();
    let account_storage = AccountStorage::new(
        vec![SlotItem {
            index: 0,
            slot: StorageSlot::new_value(public_key),
        }],
        vec![],
    )
    .unwrap();

    let account_vault = match assets {
        Some(asset) => AssetVault::new(&[asset]).unwrap(),
        None => AssetVault::new(&[]).unwrap(),
    };

    Account::new(
        account_id,
        account_vault,
        account_storage,
        account_code,
        Felt::new(1),
    )
}

#[cfg(test)]
pub fn get_note_with_fungible_asset_and_script(
    fungible_asset: FungibleAsset,
    note_script: ProgramAst,
) -> Note {
    let note_assembler = TransactionKernel::assembler();
    let (note_script, _) = NoteScript::new(note_script, &note_assembler).unwrap();
    const SERIAL_NUM: Word = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];
    let sender_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    let vault = NoteAssets::new(vec![fungible_asset.into()]).unwrap();
    let metadata = NoteMetadata::new(sender_id, NoteType::Public, 1.into(), ZERO).unwrap();
    let inputs = NoteInputs::new(vec![]).unwrap();
    let recipient = NoteRecipient::new(SERIAL_NUM, note_script, inputs);

    Note::new(vault, metadata, recipient)
} */

pub const DEFAULT_ACCOUNT_CODE: &str = "
    use.miden::contracts::wallets::basic->basic_wallet
    use.miden::contracts::auth::basic->basic_eoa

    export.basic_wallet::receive_asset
    export.basic_wallet::send_asset
    export.basic_eoa::auth_tx_rpo_falcon512
";

pub fn get_new_pk_and_authenticator() -> (
    Word,
    std::rc::Rc<miden_tx::auth::BasicAuthenticator<rand::rngs::StdRng>>,
) {
    use std::rc::Rc;

    use miden_objects::accounts::AuthSecretKey;
    use miden_tx::auth::BasicAuthenticator;
    use rand::rngs::StdRng;

    let seed = [0_u8; 32];
    let mut rng = ChaCha20Rng::from_seed(seed);

    let sec_key = SecretKey::with_rng(&mut rng);
    let pub_key: Word = sec_key.public_key().into();

    let authenticator =
        BasicAuthenticator::<StdRng>::new(&[(pub_key, AuthSecretKey::RpoFalcon512(sec_key))]);

    (pub_key, Rc::new(authenticator))
}

pub fn get_account_with_default_account_code(
    account_id: AccountId,
    public_key: Word,
    assets: Option<Asset>,
) -> Account {
    use std::collections::BTreeMap;

    // use miden_objects::testing::account_code::DEFAULT_ACCOUNT_CODE;
    let account_code_src = DEFAULT_ACCOUNT_CODE;
    let account_code_ast = ModuleAst::parse(account_code_src).unwrap();
    let account_assembler = TransactionKernel::assembler();

    let account_code = AccountCode::new(account_code_ast.clone(), &account_assembler).unwrap();
    let account_storage =
        AccountStorage::new(vec![SlotItem::new_value(0, 0, public_key)], BTreeMap::new()).unwrap();

    let account_vault = match assets {
        Some(asset) => AssetVault::new(&[asset]).unwrap(),
        None => AssetVault::new(&[]).unwrap(),
    };

    Account::from_parts(
        account_id,
        account_vault,
        account_storage,
        account_code,
        Felt::new(1),
    )
}
