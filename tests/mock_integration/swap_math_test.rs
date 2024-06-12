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

fn format_value_with_decimals(value: i64, decimals: u32) -> i64 {
    value * 10i64.pow(decimals)
}

#[test]
pub fn test_swap_math_base8_large_amounts() {
    // Instantiate the assembler
    let assembler = TransactionKernel::assembler().with_debug_mode(true);

    // Values to be formatted
    let amount_a = format_value_with_decimals(100000, 8);
    let amount_b = format_value_with_decimals(990000, 8);

    let assembly_code = format!(
        "
  use.std::math::u64

  begin

    push.{amount_a}

    # scale by 1e5
    push.100000
    mul
    
    u32split

    push.{amount_b}

    u32split
    # => [b_hi, b_lo, a_hi, a_lo]
    
    exec.u64::div

    # convert u64 to single stack => must be less than 2**64 - 2**32
    # u64_number = (high_part * (2**32)) + low_part
  
    push.4294967296 mul add

  end
",
        amount_a = amount_a,
        amount_b = amount_b
    );

    // Compile the program from the loaded assembly code
    let program = assembler
        .compile(assembly_code)
        .expect("Failed to compile the assembly code");

    let stack_inputs = StackInputs::try_from_ints([]).unwrap();

    let host = DefaultHost::default();

    // Execute the program and generate a STARK proof
    let (outputs, _proof) = prove(&program, stack_inputs, host, ProvingOptions::default())
        .expect("Failed to execute the program and generate a proof");

    println!("outputs: {:?}", outputs);

    // Get the result from the assembly output and convert to i64
    let assembly_result_u64: u64 = outputs.stack()[0].into();
    let assembly_result: i64 = assembly_result_u64 as i64;

    // Compute the expected result in Rust with fixed-point precision taken into account
    let rust_result = (amount_a as f64 / amount_b as f64 * 10f64.powi(5)).round() as i64;

    // Define the acceptable margin
    let margin = 1000; // Adjust the margin as needed

    println!("assembly_result: {}", assembly_result);
    println!("rust_result: {}", rust_result);

    // Check if the results match within the margin
    assert!(
        (assembly_result - rust_result).abs() <= margin,
        "Assembly result ({}) and Rust result ({}) differ by more than {}",
        assembly_result,
        rust_result,
        margin
    );
}

#[test]
pub fn test_swap_math_base8_small_amounts() {
    // Instantiate the assembler
    let assembler = TransactionKernel::assembler().with_debug_mode(true);

    // Values to be formatted
    let amount_a = format_value_with_decimals(1, 8);
    let amount_b = format_value_with_decimals(70000, 8);

    let assembly_code = format!(
        "
      use.std::math::u64

      begin

        push.{amount_a}

        # scale by 1e5
        push.100000
        mul
        
        u32split

        push.{amount_b}

        u32split
        # => [b_hi, b_lo, a_hi, a_lo]
        
        exec.u64::div

        # convert u64 to single stack => must be less than 2**64 - 2**32
        # u64_number = (high_part * (2**32)) + low_part
      
        push.4294967296 mul add

      end
    ",
        amount_a = amount_a,
        amount_b = amount_b
    );

    // Compile the program from the loaded assembly code
    let program = assembler
        .compile(assembly_code)
        .expect("Failed to compile the assembly code");

    let stack_inputs = StackInputs::try_from_ints([]).unwrap();

    let host = DefaultHost::default();

    // Execute the program and generate a STARK proof
    let (outputs, _proof) = prove(&program, stack_inputs, host, ProvingOptions::default())
        .expect("Failed to execute the program and generate a proof");

    println!("outputs: {:?}", outputs);

    // Get the result from the assembly output and convert to i64
    let assembly_result_u64: u64 = outputs.stack()[0].into();
    let assembly_result: i64 = assembly_result_u64 as i64;

    // Compute the expected result in Rust with fixed-point precision taken into account
    let rust_result = (amount_a as f64 / amount_b as f64 * 10f64.powi(5)).round() as i64;

    // Define the acceptable margin
    let margin = 1000; // Adjust the margin as needed

    println!("assembly_result: {}", assembly_result);
    println!("rust_result: {}", rust_result);

    // Check if the results match within the margin
    assert!(
        (assembly_result - rust_result).abs() <= margin,
        "Assembly result ({}) and Rust result ({}) differ by more than {}",
        assembly_result,
        rust_result,
        margin
    );
}

#[test]
pub fn test_fixed_point_div() {
    // Instantiate the assembler
    let assembler = TransactionKernel::assembler().with_debug_mode(true);

    // Values to be formatted
    let amount_a = format_value_with_decimals(1, 8);
    let amount_b = format_value_with_decimals(70000, 8);

    let assembly_code = format!(
        "
      use.std::math::u64

      begin
        push.{amount_a}
        dup
        mem_store.3
        u32split

        push.{amount_b}
        dup
        mem_store.4
        u32split

        exec.u64::div

        push.4294967296 mul add

        # save result to mem address 0
        mem_store.0

        mem_load.0
        push.1000000
        lt
        if.true
          # scale 1e5
          push.100000
          mem_store.1
        else end

        mem_load.0
        push.100000
        lt
        if.true
          # scale 1e6
          push.1000000
          mem_store.1
        else end

        mem_load.0
        push.10000
        lt
        if.true
          # scale 1e7
          push.10000000
          mem_store.1
        else end

        mem_load.0
        push.1000
        lt
        if.true
          # scale 1e8
          push.100000000
          mem_store.1
        else end

        mem_load.0
        push.100
        lt
        if.true
          # scale 1e9
          push.1000000000
          mem_store.1
        else end

        mem_load.0
        push.10
        lt
        if.true
          # scale 1e10
          push.10000000000
          mem_store.1
        else end

        mem_load.0
        push.1
        lt
        if.true
          # scale 1e11
          push.100000000000
          mem_store.1
        else end

        # a * scale factor

        debug.mem

        mem_load.1
        mem_load.3

        mul

        u32split

        mem_load.4
        u32split

        exec.u64::div
        

      end
    ",
        amount_a = amount_a,
        amount_b = amount_b
    );

    // Compile the program from the loaded assembly code
    let program = assembler
        .compile(assembly_code)
        .expect("Failed to compile the assembly code");

    let stack_inputs = StackInputs::try_from_ints([]).unwrap();

    let host = DefaultHost::default();

    // Execute the program and generate a STARK proof
    let (outputs, _proof) = prove(&program, stack_inputs, host, ProvingOptions::default())
        .expect("Failed to execute the program and generate a proof");

    println!("outputs: {:?}", outputs);

    // Get the result from the assembly output and convert to i64
    let assembly_result_u64: u64 = outputs.stack()[0].into();
    let assembly_result: i64 = assembly_result_u64 as i64;

    // Compute the expected result in Rust with fixed-point precision taken into account
    let rust_result = (amount_a as f64 / amount_b as f64 * 10f64.powi(5)).round() as i64;

    // Define the acceptable margin
    let margin = 1000; // Adjust the margin as needed

    println!("assembly_result: {}", assembly_result);
    println!("rust_result: {}", rust_result);

    // Check if the results match within the margin
    /*     assert!(
        (assembly_result - rust_result).abs() <= margin,
        "Assembly result ({}) and Rust result ({}) differ by more than {}",
        assembly_result,
        rust_result,
        margin
    ); */
}

#[test]
pub fn test_swap_math_base8_conversion() {
    // Instantiate the assembler
    let assembler = TransactionKernel::assembler().with_debug_mode(true);

    let assembly_code = include_str!("../../src/test/u64_swap_math.masm");

    // Compile the program from the loaded assembly code
    let program = assembler
        .compile(assembly_code)
        .expect("Failed to compile the assembly code");

    let stack_inputs = StackInputs::try_from_ints([]).unwrap();

    let host = DefaultHost::default();

    // Execute the program and generate a STARK proof
    let (outputs, _proof) = prove(&program, stack_inputs, host, ProvingOptions::default())
        .expect("Failed to execute the program and generate a proof");

    println!("outputs: {:?}", outputs);

    let result = outputs.stack().get(0).unwrap();

    println!("result: {:?}", result);
}
