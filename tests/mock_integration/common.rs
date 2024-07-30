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

use miden_lib::notes::utils::build_p2id_recipient;
use miden_objects::{
    accounts::StorageSlot,
    assembly::{AssemblyContext, ProgramAst},

    crypto::hash::rpo::RpoDigest,
    notes::{
        Note, NoteAssets, NoteDetails, NoteExecutionHint, NoteInputs, NoteMetadata,
        NoteRecipient, NoteScript, NoteTag, NoteType,
    },
    
    vm::CodeBlock,
    NoteError, 
};
use miden_vm::Assembler;
use std::collections::BTreeMap;

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
);
// REGULAR ACCOUNTS - ON-CHAIN
pub const ACCOUNT_ID_REGULAR_ACCOUNT_IMMUTABLE_CODE_ON_CHAIN: u64 = account_id(
    AccountType::RegularAccountImmutableCode,
    AccountStorageType::OnChain,
    0b0001_1111,
);
pub const ACCOUNT_ID_REGULAR_ACCOUNT_IMMUTABLE_CODE_ON_CHAIN_2: u64 = account_id(
    AccountType::RegularAccountImmutableCode,
    AccountStorageType::OnChain,
    0b0010_1111,
);
pub const ACCOUNT_ID_REGULAR_ACCOUNT_UPDATABLE_CODE_ON_CHAIN: u64 = account_id(
    AccountType::RegularAccountUpdatableCode,
    AccountStorageType::OnChain,
    0b0011_1111,
);
pub const ACCOUNT_ID_REGULAR_ACCOUNT_UPDATABLE_CODE_ON_CHAIN_2: u64 = account_id(
    AccountType::RegularAccountUpdatableCode,
    AccountStorageType::OnChain,
    0b0100_1111,
);

// FUNGIBLE TOKENS - OFF-CHAIN
pub const ACCOUNT_ID_FUNGIBLE_FAUCET_OFF_CHAIN: u64 = account_id(
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

pub fn get_custom_account_code(
    account_id: AccountId,
    public_key: Word,
    assets: Option<Asset>,
) -> Account {
    let account_code_src = include_str!("../../src/accounts/user_wallet.masm");
    let account_code_ast = ModuleAst::parse(account_code_src).unwrap();
    let account_assembler = TransactionKernel::assembler().with_debug_mode(true);

    let account_code = AccountCode::new(account_code_ast.clone(), &account_assembler).unwrap();

    let account_storage = AccountStorage::new(
        vec![SlotItem {
            index: 0,
            slot: StorageSlot::new_value(public_key),
        }],
        BTreeMap::new(),
    )
    .unwrap();

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

pub fn build_swap_tag(
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

pub fn create_p2id_output_note(
    creator: AccountId,
    swap_serial_num: [Felt; 4],
    fill_number: u64,
) -> Result<(NoteRecipient, Word), NoteError> {
    let p2id_serial_num: Word = NoteInputs::new(vec![
        swap_serial_num[0],
        swap_serial_num[1],
        swap_serial_num[2],
        swap_serial_num[3],
        Felt::new(fill_number),
    ])?
    .commitment()
    .into();

    let payback_recipient = build_p2id_recipient(creator, p2id_serial_num)?;

    Ok((payback_recipient, p2id_serial_num))
}

pub fn create_partial_swap_note(
    creator: AccountId,
    last_consumer: AccountId,
    offered_asset: Asset,
    requested_asset: Asset,
    note_type: NoteType,
    swap_serial_num: [Felt; 4],
    fill_number: u64,
) -> Result<(Note, NoteDetails, RpoDigest), NoteError> {
    let note_code = include_str!("../../src/notes/SWAPp.masm");
    let (note_script, _code_block) = new_note_script(
        ProgramAst::parse(note_code).unwrap(),
        &TransactionKernel::assembler().with_debug_mode(true),
    )
    .unwrap();

    let (payback_recipient, _p2id_serial_num) =
        create_p2id_output_note(creator, swap_serial_num, fill_number).unwrap();

    let payback_recipient_word: Word = payback_recipient.digest().into();
    let requested_asset_word: Word = requested_asset.into();

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
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
        Felt::new(fill_number),
        Felt::new(0),
        Felt::new(0),
        Felt::new(0),
        creator.into(),
    ])?;

    let offered_asset_amount: Word = offered_asset.into();
    let aux = offered_asset_amount[0];

    // build the outgoing note
    let metadata = NoteMetadata::new(last_consumer, note_type, tag, aux)?;
    let assets = NoteAssets::new(vec![offered_asset])?;
    let recipient = NoteRecipient::new(swap_serial_num, note_script.clone(), inputs.clone());
    let note = Note::new(assets.clone(), metadata, recipient.clone());

    // build the payback note details
    let payback_assets = NoteAssets::new(vec![requested_asset])?;
    let payback_note = NoteDetails::new(payback_assets, payback_recipient);

    let note_script_hash = note_script.hash();

    Ok((note, payback_note, note_script_hash))
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

pub fn format_value_with_decimals(value: u64, decimals: u32) -> u64 {
    value * 10u64.pow(decimals)
}

pub fn format_value_to_float(value: u64, decimals: u32) -> f32 {
    let scale = 10f32.powi(decimals as i32);
    let result: f32 = ((value as f32) / scale) as f32;
    result
}


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
