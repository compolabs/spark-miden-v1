use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{
        account_id::testing::ACCOUNT_ID_SENDER, Account, AccountCode, AccountId, AccountStorage,
        SlotItem,
    },
    assets::{Asset, AssetVault, FungibleAsset},
    crypto::hash::rpo::RpoDigest,
    crypto::rand::FeltRng,
    crypto::{dsa::rpo_falcon512::SecretKey, utils::Serializable},
    notes::{
        Note, NoteAssets, NoteDetails, NoteExecutionHint, NoteExecutionMode, NoteInputs,
        NoteMetadata, NoteRecipient, NoteScript, NoteTag, NoteType,
    },
    testing::account_code::DEFAULT_AUTH_SCRIPT,
    transaction::{ExecutedTransaction, ProvenTransaction, TransactionArgs, TransactionScript},
    vm::Program,
    Felt, NoteError, Word, ZERO,
};
use miden_prover::ProvingOptions;
use miden_tx::{TransactionProver, TransactionVerifier, TransactionVerifierError};
use miden_vm::Assembler;
use rand_chacha::{rand_core::SeedableRng, ChaCha20Rng};
use vm_processor::utils::Deserializable;

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

#[cfg(test)]
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

#[cfg(test)]
pub fn get_account_with_default_account_code(
    account_id: AccountId,
    public_key: Word,
    assets: Option<Asset>,
) -> Account {
    use std::collections::BTreeMap;

    use miden_objects::testing::account_code::DEFAULT_ACCOUNT_CODE;
    let account_code_src = DEFAULT_ACCOUNT_CODE;
    let assembler = TransactionKernel::assembler().with_debug_mode(true);

    let account_code = AccountCode::compile(account_code_src, assembler).unwrap();
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

#[cfg(test)]
pub fn get_note_with_fungible_asset_and_script(
    fungible_asset: FungibleAsset,
    note_script: &str,
) -> Note {
    use miden_objects::notes::NoteExecutionHint;

    let assembler = TransactionKernel::assembler().with_debug_mode(true);
    let note_script = NoteScript::compile(note_script, assembler).unwrap();
    const SERIAL_NUM: Word = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];
    let sender_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    let vault = NoteAssets::new(vec![fungible_asset.into()]).unwrap();
    let metadata = NoteMetadata::new(
        sender_id,
        NoteType::Public,
        1.into(),
        NoteExecutionHint::Always,
        ZERO,
    )
    .unwrap();
    let inputs = NoteInputs::new(vec![]).unwrap();
    let recipient = NoteRecipient::new(SERIAL_NUM, note_script, inputs);

    Note::new(vault, metadata, recipient)
}

#[cfg(test)]
pub fn build_default_auth_script() -> TransactionScript {
    TransactionScript::compile(DEFAULT_AUTH_SCRIPT, [], TransactionKernel::assembler()).unwrap()
}

#[cfg(test)]
pub fn build_tx_args_from_script(script_source: &str) -> TransactionArgs {
    let tx_script =
        TransactionScript::compile(script_source, [], TransactionKernel::assembler()).unwrap();
    TransactionArgs::with_tx_script(tx_script)
}

/// Creates a [NoteRecipient] for the P2ID note.
///
/// Notes created with this recipient will be P2ID notes consumable by the specified target
/// account.
pub fn build_p2id_recipient(
    target: AccountId,
    serial_num: Word,
) -> Result<NoteRecipient, NoteError> {
    let assembler: Assembler = TransactionKernel::assembler_testing().with_debug_mode(true);
    let note_code = include_str!("../../src/notes/P2ID.masm");
    let note_script = NoteScript::compile(note_code, assembler).unwrap();

    let note_inputs = NoteInputs::new(vec![target.into()])?;

    Ok(NoteRecipient::new(serial_num, note_script, note_inputs))
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

    let execution = NoteExecutionMode::Local;
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

    println!("HERE");
    let payback_recipient = build_p2id_recipient(creator, p2id_serial_num)?;
    println!("here");
    Ok((payback_recipient, p2id_serial_num))
}

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
    note_type: NoteType,
    swap_serial_num: [Felt; 4],
    fill_number: u64,
) -> Result<(Note, NoteDetails, RpoDigest), NoteError> {
    let assembler: Assembler = TransactionKernel::assembler_testing().with_debug_mode(true);

    let note_code = include_str!("../../src/notes/SWAPp.masm");

    println!("before compile");

    let note_script = NoteScript::compile(note_code, assembler).unwrap();

    println!("after compile");

    // println!("notescript: {:?}", note_script.mast());

    let (payback_recipient, _p2id_serial_num) =
        create_p2id_output_note(creator, swap_serial_num, fill_number).unwrap();

    println!("after p2id");

    let requested_asset_word: Word = requested_asset.into();
    let tag = build_swap_tag(note_type, &offered_asset, &requested_asset)?;

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

    println!("inputs: {:?}", inputs.values());

    // let offered_asset_amount: Word = offered_asset.into();
    let aux = Felt::new(27);

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

    // build the payback note details
    let payback_assets = NoteAssets::new(vec![requested_asset])?;
    let payback_note = NoteDetails::new(payback_assets, payback_recipient);

    let note_script_hash = note_script.hash();

    Ok((note, payback_note, note_script_hash))
}
