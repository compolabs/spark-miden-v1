use crate::utils::{
    MockDataStore, ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN, ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1,
    ACCOUNT_ID_REGULAR_ACCOUNT_IMMUTABLE_CODE_ON_CHAIN, ACCOUNT_ID_SENDER,
};
use miden_lib::notes::utils::build_p2id_recipient;
use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{Account, AccountCode, AccountId, AccountStorage, SlotItem, StorageSlot},
    assembly::{AssemblyContext, ModuleAst, ProgramAst},
    assets::{Asset, AssetVault, FungibleAsset},
    crypto::{
        hash::rpo::RpoDigest,
        rand::{FeltRng, RpoRandomCoin},
    },
    notes::{
        Note, NoteAssets, NoteDetails, NoteExecutionHint, NoteHeader, NoteInputs, NoteMetadata,
        NoteRecipient, NoteScript, NoteTag, NoteType,
    },
    transaction::{InputNotes, TransactionArgs},
    vm::CodeBlock,
    Digest, Felt, FieldElement, Hasher, NoteError, Word, ZERO,
};
use miden_vm::{prove, verify, Assembler, DefaultHost, ProvingOptions, StackInputs};

use miden_objects::crypto::hash::rpo::Rpo256;

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

fn pad_inputs(inputs: &[Felt]) -> Vec<Felt> {
    const BLOCK_SIZE: usize = 4 * 2;

    let padded_len = inputs.len().next_multiple_of(BLOCK_SIZE);
    let mut padded_inputs = Vec::with_capacity(padded_len);
    padded_inputs.extend(inputs.iter());
    padded_inputs.resize(padded_len, ZERO);

    padded_inputs
}

#[test]
pub fn test_input_hash() {
    let vec_inputs = vec![
        Felt::new(1),
        Felt::new(2),
        Felt::new(3),
        Felt::new(4),
        Felt::new(5),
        Felt::new(6),
        Felt::new(7),
        Felt::new(8),
        Felt::new(9),
    ];

    let inputs = NoteInputs::new(vec_inputs.clone()).unwrap();
    // println!("input commitment : {:?}", inputs.commitment());

    let padded_values = pad_inputs(&vec_inputs);
    println!("padded values: {:?}", padded_values);

    let inputs_commitment = Hasher::hash_elements(&padded_values);
    println!("inputs commitment: {:?}", inputs_commitment);

    assert_eq!(inputs.commitment(), inputs_commitment);

    // ##### RUN MASM ###### //

    // Instantiate the assembler
    let assembler = Assembler::default().with_debug_mode(true);

    // Read the assembly program from a file
    let assembly_code: &str = include_str!("../../src/test/recipient_hash_test.masm");

    // Compile the program from the loaded assembly code
    let program = assembler
        .compile(assembly_code)
        .expect("Failed to compile the assembly code");

    let stack_inputs = StackInputs::try_from_ints([]).unwrap();

    let host = DefaultHost::default();

    // Execute the program and generate a STARK proof
    let (outputs, _proof) = prove(&program, stack_inputs, host, ProvingOptions::default())
        .expect("Failed to execute the program and generate a proof");

    let serial_num = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];
    let serial_num_hash = Hasher::merge(&[serial_num.into(), Digest::default()]);

    let note_script_code = include_str!("../../src/test/basic_note.masm");
    let note_script = new_note_script(ProgramAst::parse(note_script_code).unwrap(), &assembler)
        .unwrap()
        .0;

    let note_script_hash: Digest = note_script.hash();

    let serial_script_hash = Hasher::merge(&[serial_num_hash, note_script_hash]);

    let recipient_1 = Hasher::merge(&[serial_script_hash, inputs.commitment()]);
    let recipient = NoteRecipient::new(serial_num, note_script, inputs.clone());

    assert_eq!(recipient_1, recipient.digest());

    println!("Stack Output: {:?}", outputs.stack());

    /*
    let mut stack_output = outputs.stack().to_vec();
    let recipient_hash = inputs.commitment().as_slice().to_vec();

    if stack_output.len() > 8 {
        stack_output.truncate(stack_output.len() - 13);
    }

    stack_output.reverse();
    */

    // asserting that the stack output is equal to the recipient hash
    // the calculated in MASM proc equals what was calculated in Rust
    // assert_eq!(stack_output, recipient_hash);
    // println!("Stack Output: {:?}", stack_output);

    // assert_eq!(inputs.commitment(), outputs.stack());

    // verify(program.into(), cloned_inputs, outputs, proof).unwrap();

    // Ok((note, payback_note, note_script_hash))
}
