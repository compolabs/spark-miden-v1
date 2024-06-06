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
    crypto::hash::rpo::RpoDigest,
    crypto::rand::{FeltRng, RpoRandomCoin},
    notes::{
        Note, NoteAssets, NoteDetails, NoteExecutionHint, NoteHeader, NoteInputs, NoteMetadata,
        NoteRecipient, NoteScript, NoteTag, NoteType,
    },
    transaction::TransactionArgs,
    vm::CodeBlock,
    Digest, Felt, Hasher, NoteError, Word, ZERO,
};
use miden_vm::{prove, verify, Assembler, DefaultHost, ProvingOptions, StackInputs};

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

#[test]
pub fn create_partial_swap_note() {
    // SWAPp note creator
    let sender = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();

    // Offered Asset
    let faucet_id = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN).unwrap();
    let offered_asset: Asset = FungibleAsset::new(faucet_id, 100).unwrap().into();

    // Requested Asset
    let faucet_id_2 = AccountId::try_from(ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN_1).unwrap();
    let requested_asset: Asset = FungibleAsset::new(faucet_id_2, 100).unwrap().into();

    let note_type = NoteType::Public;
    let mut rng = RpoRandomCoin::new([Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)]);

    let note_code = include_str!("../../src/test/basic_note.masm");
    let (note_script, _code_block) = new_note_script(
        ProgramAst::parse(note_code).unwrap(),
        &TransactionKernel::assembler().with_debug_mode(true),
    )
    .unwrap();

    let payback_serial_num = rng.draw_word();
    let payback_recipient = build_p2id_recipient(sender, payback_serial_num).unwrap();

    let payback_recipient_word: Word = payback_recipient.digest().into();
    let requested_asset_word: Word = requested_asset.into();
    let payback_tag = NoteTag::from_account_id(sender, NoteExecutionHint::Local).unwrap();

    println!("{:?}", payback_tag);
    let inputs = NoteInputs::new(vec![
        payback_recipient_word[0],
        payback_recipient_word[1],
        payback_recipient_word[2],
        payback_recipient_word[3],
        requested_asset_word[0],
        requested_asset_word[1],
        requested_asset_word[2],
        requested_asset_word[3],
        payback_tag.inner().into(),
    ])
    .unwrap();

    // build the tag for the SWAP use case
    let tag = build_swap_tag(note_type, &offered_asset, &requested_asset).unwrap();
    let serial_num = rng.draw_word();
    let aux = ZERO;

    // build the outgoing note
    let metadata = NoteMetadata::new(sender, note_type, tag, aux).unwrap();
    let assets = NoteAssets::new(vec![offered_asset]).unwrap();
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), inputs.clone());
    let note = Note::new(assets, metadata, recipient);

    // build the payback note details
    // let payback_assets = NoteAssets::new(vec![requested_asset]).unwrap();
    // let payback_note = NoteDetails::new(payback_assets, payback_recipient);

    let note_script_hash = note_script.hash();

    println!("note_script_hash: {:?}", note_script_hash);

    // ###########

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
    let (outputs, proof) = prove(&program, stack_inputs, host, ProvingOptions::default())
        .expect("Failed to execute the program and generate a proof");

    // let inputs = NoteInputs::new(vec![Felt::new(2)]).unwrap();

    let serial_num = [Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)];
    let serial_num_hash = Hasher::merge(&[serial_num.into(), Digest::default()]);

    let note_script_code = include_str!("../../src/test/basic_note.masm");
    let note_script = new_note_script(ProgramAst::parse(note_script_code).unwrap(), &assembler)
        .unwrap()
        .0;

    let note_script_hash: Digest = note_script.hash();

    let serial_script_hash = Hasher::merge(&[serial_num_hash, note_script_hash]);

    let recipient_1 = Hasher::merge(&[serial_script_hash, inputs.commitment()]);
    let recipient = NoteRecipient::new(serial_num, note_script, inputs);

    assert_eq!(recipient_1, recipient.digest());

    println!("Stack Output: {:?}", outputs.stack());
    print!("Recipient: {:?}", recipient.digest());

    /*     let mut stack_output = outputs.stack().to_vec();
       let recipient_hash = recipient.digest().as_slice().to_vec();

       if stack_output.len() > 8 {
           stack_output.truncate(stack_output.len() - 13);
       }

       stack_output.reverse();

       // asserting that the stack output is equal to the recipient hash
       // the calculated in MASM proc equals what was calculated in Rust
       // assert_eq!(stack_output, recipient_hash);
       println!("Stack Output: {:?}", stack_output);
    */
    // verify(program.into(), cloned_inputs, outputs, proof).unwrap();

    // Ok((note, payback_note, note_script_hash))
}
