use miden_lib::transaction::TransactionKernel;
use miden_vm::{prove, DefaultHost, ProvingOptions, StackInputs};

use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestRunner};

fn format_value_with_decimals(value: u64, decimals: u32) -> u64 {
    value * 10u64.pow(decimals)
}

#[test]
pub fn test_swap_math_base8_large_amounts() {
    // Instantiate the assembler
    let assembler = TransactionKernel::assembler().with_debug_mode(true);

    // Values to be formatted
    let amount_a = format_value_with_decimals(1844674, 8);
    let amount_b = format_value_with_decimals(1902, 8);

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
pub fn test_msb_masm() {
    // Instantiate the assembler
    let assembler = TransactionKernel::assembler().with_debug_mode(true);

    let value = 15; // Binary: 1111

    let assembly_code = format!(
        "
        use.std::math::u64

        # input: [u64 a_hi, u64 a_lo]
        proc.u64_msb
            mem_store.10
            mem_store.11

            push.0
            mem_store.20

            push.1

            while.true
                mem_load.11
                mem_load.10

                push.1

                exec.u64::shr

                mem_store.10
                mem_store.11

                mem_load.20
                push.1
                add 
                mem_store.20

                mem_load.11
                push.0
                mem_load.10
                push.0

                neq neq neq

                if.true
                    push.1
                else
                    push.0
                end

            end

            mem_load.20
            push.1
            sub

        end

        begin
            push.{value}
            u32split

            exec.u64_msb
            
            push.111
            debug.stack
            drop
        end
        ",
        value = value,
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
    println!("assembly_result: {}", assembly_result);

    // Verify the result manually as per the Python function logic
    let expected_msb = if value == 0 {
        -1
    } else {
        let mut msb = 0;
        let mut n = value;
        while n != 0 {
            n >>= 1;
            msb += 1;
        }
        msb - 1
    };

    assert_eq!(
        assembly_result, expected_msb,
        "MSB calculation is incorrect"
    );
}

#[test]
pub fn test_closest_base_ten_masm() {
    // Instantiate the assembler
    let assembler = TransactionKernel::assembler().with_debug_mode(true);

    // Values to be formatted
    let value = format_value_with_decimals(103, 8);

    let assembly_code = format!(
        "
    proc.closest_base_ten

      mem_store.10
  
      # 1e18
      mem_load.10
      push.0x0DE0B6B3A7640000
      gte
      if.true
          push.0x0DE0B6B3A7640000
          mem_store.11
      else
          # 1e17
          mem_load.10
          push.0x016345785D8A0000
          gte
          if.true
              push.0x016345785D8A0000
              mem_store.11
          else
              # 1e16
              mem_load.10
              push.0x002386F26FC10000
              gte
              if.true
                  push.0x002386F26FC10000
                  mem_store.11
              else
                  # 1e15
                  mem_load.10
                  push.0x00038D7EA4C68000
                  gte
                  if.true
                      push.0x00038D7EA4C68000
                      mem_store.11
                  else
                      # 1e14
                      mem_load.10
                      push.0x00005AF3107A4000
                      gte
                      if.true
                          push.0x00005AF3107A4000
                          mem_store.11
                      else
                          # 1e13
                          mem_load.10
                          push.0x000009184E72A000
                          gte
                          if.true
                              push.0x000009184E72A000
                              mem_store.11
                          else
                              # 1e12
                              mem_load.10
                              push.0x000000E8D4A51000
                              gte
                              if.true
                                  push.0x000000E8D4A51000
                                  mem_store.11
                              else
                                  # 1e11
                                  mem_load.10
                                  push.0x000000174876E800
                                  gte
                                  if.true
                                      push.0x000000174876E800
                                      mem_store.11
                                  else
                                      # 1e10
                                      mem_load.10
                                      push.0x00000002540BE400
                                      gte
                                      if.true
                                          push.0x00000002540BE400
                                          mem_store.11
                                      else
                                          # 1e9
                                          mem_load.10
                                          push.0x3B9ACA00
                                          gte
                                          if.true
                                              push.0x3B9ACA00
                                              mem_store.11
                                          else
                                              # 1e8
                                              mem_load.10
                                              push.0x05F5E100
                                              gte
                                              if.true
                                                  push.0x05F5E100
                                                  mem_store.11
                                              else
                                                  # 1e7
                                                  mem_load.10
                                                  push.0x00989680
                                                  gte
                                                  if.true
                                                      push.0x00989680
                                                      mem_store.11
                                                  else
                                                      # 1e6
                                                      mem_load.10
                                                      push.0x000F4240
                                                      gte
                                                      if.true
                                                          push.0x000F4240
                                                          mem_store.11
                                                      else
                                                          # 1e5
                                                          mem_load.10
                                                          push.0x000186A0
                                                          gte
                                                          if.true
                                                              push.0x000186A0
                                                              mem_store.11
                                                          else
                                                              # 1e4
                                                              mem_load.10
                                                              push.0x2710
                                                              gte
                                                              if.true
                                                                  push.0x2710
                                                                  mem_store.11
                                                              else
                                                                  # 1e3
                                                                  mem_load.10
                                                                  push.0x03E8
                                                                  gte
                                                                  if.true
                                                                      push.0x03E8
                                                                      mem_store.11
                                                                  else
                                                                      # 1e2
                                                                      mem_load.10
                                                                      push.0x0064
                                                                      gte
                                                                      if.true
                                                                          push.0x0064
                                                                          mem_store.11
                                                                      else
                                                                          # 1e1
                                                                          mem_load.10
                                                                          push.0x000A
                                                                          gte
                                                                          if.true
                                                                              push.0x000A
                                                                              mem_store.11
                                                                          else
                                                                              push.1
                                                                              mem_store.11
                                                                          end
                                                                      end
                                                                  end
                                                              end
                                                          end
                                                      end
                                                  end
                                              end
                                          end
                                      end
                                  end
                              end
                          end
                      end
                  end
              end
          end
      end
  
      mem_load.11
    end
    
    begin
        push.{value}
        exec.closest_base_ten
    end
    
    ",
        value = value
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
    let assembly_result: u64 = outputs.stack()[0].into();
    println!("assembly_result: {}", assembly_result);

    // Check if the result is correct
    assert_eq!(assembly_result, format_value_with_decimals(100, 8));
}

#[test]
pub fn test_calculate_tokens_a_for_b() {
    // Values to be used in the test
    // Token amounts are base 1e8
    let (tokens_a, decimals_a) = (357179, 8);
    let (tokens_b, decimals_b) = (10, 8);
    let (tokens_b_in, decimals_b_in) = (1, 8);

    // Format values with decimals
    let amount_a = format_value_with_decimals(tokens_a, decimals_a);
    let amount_b = format_value_with_decimals(tokens_b, decimals_b);
    let amount_b_in = format_value_with_decimals(tokens_b_in, decimals_b_in);

    let assembly_code = format!(
        "
      use.std::math::u64

      const.AMT_TOKENS_A=0x0064
      const.AMT_TOKENS_B=0x0065
      const.AMT_TOKENS_B_IN=0x0066
      const.RATIO=0x0067
      
      const.FACTOR=0x000186A0 # 1e5
      const.MAX_U32=0x0000000100000000

      const.MAX_SWAP_VAL=0xA7C5AC467381

      const.ERR_INVALID_SWAP_AMOUNT=0x0000000000000001
      
      # input: [tokens_a, tokens_b, tokens_b_in]
      # output: [tokens_a_out]
      proc.calculate_tokens_a_for_b
      
        mem_store.AMT_TOKENS_A # tokens_a
        mem_store.AMT_TOKENS_B # tokens_b
        mem_store.AMT_TOKENS_B_IN # tokens_b_in
      
        mem_load.AMT_TOKENS_B mem_load.AMT_TOKENS_A
        # => [tokens_a, tokens_b]

        dup.1 dup.1
        push.MAX_SWAP_VAL lt
        swap
        push.MAX_SWAP_VAL lt

        assert.err=ERR_INVALID_SWAP_AMOUNT
        assert.err=ERR_INVALID_SWAP_AMOUNT

        gt
        if.true
          mem_load.AMT_TOKENS_B
          u32split
      
          push.FACTOR
          u32split
                  
          exec.u64::wrapping_mul
      
          mem_load.AMT_TOKENS_A
          u32split
      
          exec.u64::div
          push.MAX_U32 mul add
      
          mem_store.RATIO
      
          mem_load.AMT_TOKENS_B_IN
          u32split
      
          push.FACTOR
          u32split
      
          exec.u64::wrapping_mul
      
          mem_load.RATIO
          u32split
      
          exec.u64::div
      
          push.MAX_U32 mul add          
      
        else

          mem_load.AMT_TOKENS_A
          u32split
      
          push.FACTOR
          u32split
            
          exec.u64::wrapping_mul
     
          mem_load.AMT_TOKENS_B
          u32split
     
          exec.u64::div
     
          mem_load.AMT_TOKENS_B_IN
          u32split

          exec.u64::wrapping_mul
     
          push.FACTOR
          u32split
      
          exec.u64::div

          push.MAX_U32 mul add          
      
        end
      end

      begin

        push.{amount_b_in}
        push.{amount_b}
        push.{amount_a}

        exec.calculate_tokens_a_for_b
      end
    ",
        amount_b_in = amount_b_in,
        amount_b = amount_b,
        amount_a = amount_a,
    );

    // Instantiate the assembler
    let assembler = TransactionKernel::assembler().with_debug_mode(true);

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
    let assembly_result: u64 = outputs.stack()[0].into();
    println!("assembly_result: {}", assembly_result);

    // Compute the expected result using the Rust implementation of the Python logic
    let expected_result = calculate_tokens_a_for_b(amount_a, amount_b, amount_b_in);
    println!("expected_result: {}", expected_result);

    // Assert the assembly result matches the expected result
    assert_eq!(assembly_result, expected_result);
}

fn calculate_tokens_a_for_b(tokens_a: u64, tokens_b: u64, requested_tokens_b: u64) -> u64 {
    let scaling_factor: u128 = 100_000;

    println!(
        "tokens_a: {}, tokens_b: {}, requested_tokens_b: {}",
        tokens_a, tokens_b, requested_tokens_b
    );

    let tokens_a = tokens_a as u128;
    let tokens_b = tokens_b as u128;
    let requested_tokens_b = requested_tokens_b as u128;

    if tokens_a < tokens_b {
        let scaled_ratio = tokens_b
            .checked_mul(scaling_factor)
            .and_then(|v| v.checked_div(tokens_a))
            .expect("Multiplication or division overflow");

        println!("scaled_ratio (tokens_b < tokens_a): {}", scaled_ratio);

        requested_tokens_b
            .checked_mul(scaling_factor)
            .and_then(|v| v.checked_div(scaled_ratio))
            .expect("Multiplication or division overflow") as u64
    } else {
        let scaled_ratio = tokens_a
            .checked_mul(scaling_factor)
            .and_then(|v| v.checked_div(tokens_b))
            .expect("Multiplication or division overflow");

        println!("scaled_ratio (tokens_a >= tokens_b): {}", scaled_ratio);

        let intermediate_value = scaled_ratio
            .checked_mul(requested_tokens_b)
            .expect("Multiplication overflow");

        intermediate_value
            .checked_div(scaling_factor)
            .expect("Division overflow") as u64
    }
}

#[test]
fn fuzzed_test_calculate_tokens_a_for_b() {
    let config = ProptestConfig::with_cases(50);
    let mut runner = TestRunner::new(config);

    runner
        .run(
            &(1u64..=1_800_000, 1u32..=8, 1u64..=1_800_000, 1u32..=8).prop_flat_map(
                |(tokens_a, decimals_a, tokens_b, decimals_b)| {
                    (
                        Just(tokens_a),
                        Just(decimals_a),
                        Just(tokens_b),
                        Just(decimals_b),
                        1u64..=tokens_b,
                        0u32..=decimals_b,
                    )
                },
            ),
            |(tokens_a, decimals_a, tokens_b, decimals_b, tokens_b_in, decimals_b_in)| {
                // Format values with decimals
                let amount_a = format_value_with_decimals(tokens_a, decimals_a);
                let amount_b = format_value_with_decimals(tokens_b, decimals_b);
                let amount_b_in = format_value_with_decimals(tokens_b_in, decimals_b_in);

                let assembly_code = format!(
                    "
            use.std::math::u64

            const.AMT_TOKENS_A=0x0064
            const.AMT_TOKENS_B=0x0065
            const.AMT_TOKENS_B_IN=0x0066
            const.RATIO=0x0067
            
            const.FACTOR=0x000186A0 # 1e5
            const.MAX_U32=0x0000000100000000

            const.MAX_SWAP_VAL=0xA7C5AC467381

            const.ERR_INVALID_SWAP_AMOUNT=0x0000000000000001
            
            # input: [tokens_a, tokens_b, tokens_b_in]
            # output: [tokens_a_out]
            proc.calculate_tokens_a_for_b
            
                mem_store.AMT_TOKENS_A # tokens_a
                mem_store.AMT_TOKENS_B # tokens_b
                mem_store.AMT_TOKENS_B_IN # tokens_b_in
            
                mem_load.AMT_TOKENS_B mem_load.AMT_TOKENS_A
                # => [tokens_a, tokens_b]

                dup.1 dup.1
                push.MAX_SWAP_VAL lt
                swap
                push.MAX_SWAP_VAL lt

                assert.err=ERR_INVALID_SWAP_AMOUNT
                assert.err=ERR_INVALID_SWAP_AMOUNT

                gt
                if.true
                  mem_load.AMT_TOKENS_B
                  u32split
            
                  push.FACTOR
                  u32split
                          
                  exec.u64::wrapping_mul
            
                  mem_load.AMT_TOKENS_A
                  u32split
            
                  exec.u64::div
                  push.MAX_U32 mul add
            
                  mem_store.RATIO
            
                  mem_load.AMT_TOKENS_B_IN
                  u32split
            
                  push.FACTOR
                  u32split
            
                  exec.u64::wrapping_mul
            
                  mem_load.RATIO
                  u32split
            
                  exec.u64::div
            
                  push.MAX_U32 mul add          
            
                else
                  mem_load.AMT_TOKENS_A
                  u32split
            
                  push.FACTOR
                  u32split
                          
                  exec.u64::wrapping_mul
            
                  mem_load.AMT_TOKENS_B
                  u32split
            
                  exec.u64::div
            
                  mem_load.AMT_TOKENS_B_IN
                  u32split
            
                  exec.u64::wrapping_mul
            
                  push.FACTOR
                  u32split
            
                  exec.u64::div
                  push.MAX_U32 mul add          
            
                end
            end

            begin

                push.{amount_b_in}
                push.{amount_b}
                push.{amount_a}

                exec.calculate_tokens_a_for_b
            end
            ",
                    amount_b_in = amount_b_in,
                    amount_b = amount_b,
                    amount_a = amount_a,
                );

                // Instantiate the assembler
                let assembler = TransactionKernel::assembler().with_debug_mode(true);

                // Compile the program from the loaded assembly code
                let program = assembler
                    .compile(assembly_code)
                    .expect("Failed to compile the assembly code");

                let stack_inputs = StackInputs::try_from_ints([]).unwrap();

                let host = DefaultHost::default();

                // Execute the program and generate a STARK proof
                let (outputs, _proof) =
                    prove(&program, stack_inputs, host, ProvingOptions::default())
                        .expect("Failed to execute the program and generate a proof");

                println!("outputs: {:?}", outputs);

                // Get the result from the assembly output and convert to i64
                let assembly_result: u64 = outputs.stack()[0].into();
                println!("assembly_result: {}", assembly_result);

                // Compute the expected result using the Rust implementation of the Python logic
                let expected_result = calculate_tokens_a_for_b(amount_a, amount_b, amount_b_in);
                println!("expected_result: {}", expected_result);

                // Assert the assembly result matches the expected result
                assert_eq!(assembly_result, expected_result);

                Ok(())
            },
        )
        .expect("Test failed");
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
