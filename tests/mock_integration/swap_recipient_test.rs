use miden_objects::{
    assembly::{AssemblyContext, ProgramAst},
    notes::{NoteInputs, NoteRecipient, NoteScript},
    vm::CodeBlock,
    Digest, Felt, Hasher, NoteError, ZERO,
};
use miden_vm::{prove, Assembler, DefaultHost, ProvingOptions, StackInputs};

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
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), inputs.clone());

    assert_eq!(recipient_1, recipient.digest());

    let serial_num_hash = Hasher::merge(&[serial_num.into(), Digest::default()]);
    let merge_script = Hasher::merge(&[serial_num_hash, note_script.hash()]);
    let recipient_hash = Hasher::merge(&[merge_script, inputs.commitment()]);

    println!("Serial Num Hash: {:?}", serial_num_hash);
    println!("Merge Script Hash: {:?}", merge_script);
    println!("Recipient Hash: {:?}", recipient_hash);

    println!("inputs commitment: {:?}", inputs.commitment());
    println!("note_script_hash : {:?}", note_script_hash);
    println!("serial_script_hash: {:?}", serial_script_hash);
    println!("Recipient Hash: {:?}", recipient.digest());
    println!("Stack Output: {:?}", outputs.stack());
}
