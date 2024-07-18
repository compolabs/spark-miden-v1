use miden_vm::{prove, Assembler, DefaultHost, ProvingOptions, StackInputs};

#[test]
pub fn test_revert_procedure() {
    // Instantiate the assembler
    let assembler = Assembler::default().with_debug_mode(true);

    // Read the assembly program from a file
    let assembly_code: &str = include_str!("../../src/test/revert_procedure.masm");

    // Compile the program from the loaded assembly code
    let program = assembler
        .compile(assembly_code)
        .expect("Failed to compile the assembly code");

    let stack_inputs = StackInputs::try_from_ints([]).unwrap();

    let host = DefaultHost::default();

    // Expect revert
    prove(&program, stack_inputs, host, ProvingOptions::default()).expect_err("Failed to revert");
}
