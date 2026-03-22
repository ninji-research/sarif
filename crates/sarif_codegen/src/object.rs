use std::collections::BTreeMap;

use cranelift_codegen::ir::{AbiParam, InstBuilder, Signature, UserFuncName, types};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::native::{
    F64VecHeader, NativeEnum, NativeRecord, NativeValueRepr, TrustedF64VecAccesses,
    collect_native_enums, collect_native_records, declare_arg_count, declare_arg_text,
    declare_f64_vec_new, declare_parse_i32, declare_record_allocator, declare_stdin_text,
    declare_stdout_write, declare_text_builder_append, declare_text_builder_finish,
    declare_text_builder_new, declare_text_concat, declare_text_data_for_insts, declare_text_eq,
    declare_text_from_f64_fixed, declare_text_slice, encode_text_blob, infer_value_kinds,
    lower_insts, native_type as shared_native_type, value_repr as shared_value_repr,
};
use crate::{Function, Program, ValueId};

pub const ENTRYPOINT_SYMBOL: &str = "sarif_user_main";

#[derive(Debug)]
pub struct ObjectError {
    pub message: String,
}

impl ObjectError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

type ValueRepr = NativeValueRepr;

/// # Errors
///
/// Returns an error if object emission fails, if the stage-0 object backend
/// cannot represent one of the program's value types, or if Cranelift rejects
/// the generated CLIF.
pub fn emit_object(program: &Program, module_name: &str) -> Result<Vec<u8>, ObjectError> {
    let mut backend = ObjectBackend::new(program, module_name)?;
    backend.emit()
}

struct ObjectBackend<'a> {
    program: &'a Program,
    module: Option<ObjectModule>,
    function_ids: BTreeMap<String, FuncId>,
    data_ids: BTreeMap<String, DataId>,
    allocator_id: FuncId,
    text_builder_new_id: FuncId,
    text_builder_append_id: FuncId,
    text_builder_finish_id: FuncId,
    f64_vec_new_id: FuncId,
    text_concat_id: FuncId,
    text_slice_id: FuncId,
    text_from_f64_fixed_id: FuncId,
    parse_i32_id: FuncId,
    arg_count_id: FuncId,
    arg_text_id: FuncId,
    stdin_text_id: FuncId,
    stdout_write_id: FuncId,
    text_eq_id: FuncId,
    records: BTreeMap<String, NativeRecord>,
    native_enums: BTreeMap<String, NativeEnum>,
}

impl<'a> ObjectBackend<'a> {
    fn new(program: &'a Program, module_name: &str) -> Result<Self, ObjectError> {
        let mut flag_builder = settings::builder();
        flag_builder.set("opt_level", "speed").map_err(|error| {
            ObjectError::new(format!("failed to set cranelift opt_level: {error}"))
        })?;
        flag_builder
            .set("regalloc_algorithm", "backtracking")
            .map_err(|error| {
                ObjectError::new(format!(
                    "failed to set cranelift regalloc_algorithm: {error}"
                ))
            })?;
        let isa_builder = cranelift_native::builder()
            .map_err(|error| ObjectError::new(format!("failed to build native ISA: {error}")))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|error| ObjectError::new(format!("failed to finish native ISA: {error}")))?;
        let builder = ObjectBuilder::new(isa, module_name, Box::new(default_libcall_names()))
            .map_err(|error| {
                ObjectError::new(format!(
                    "failed to create object builder `{module_name}`: {error}"
                ))
            })?;
        let mut module = ObjectModule::new(builder);
        let allocator_id =
            declare_record_allocator(&mut module, "object").map_err(ObjectError::new)?;
        let text_builder_new_id =
            declare_text_builder_new(&mut module, "object").map_err(ObjectError::new)?;
        let text_builder_append_id =
            declare_text_builder_append(&mut module, "object").map_err(ObjectError::new)?;
        let text_builder_finish_id =
            declare_text_builder_finish(&mut module, "object").map_err(ObjectError::new)?;
        let f64_vec_new_id =
            declare_f64_vec_new(&mut module, "object").map_err(ObjectError::new)?;
        let text_concat_id =
            declare_text_concat(&mut module, "object").map_err(ObjectError::new)?;
        let text_slice_id = declare_text_slice(&mut module, "object").map_err(ObjectError::new)?;
        let text_from_f64_fixed_id =
            declare_text_from_f64_fixed(&mut module, "object").map_err(ObjectError::new)?;
        let parse_i32_id = declare_parse_i32(&mut module, "object").map_err(ObjectError::new)?;
        let arg_count_id = declare_arg_count(&mut module, "object").map_err(ObjectError::new)?;
        let arg_text_id = declare_arg_text(&mut module, "object").map_err(ObjectError::new)?;
        let stdin_text_id = declare_stdin_text(&mut module, "object").map_err(ObjectError::new)?;
        let stdout_write_id =
            declare_stdout_write(&mut module, "object").map_err(ObjectError::new)?;
        let text_eq_id = declare_text_eq(&mut module, "object").map_err(ObjectError::new)?;

        Ok(Self {
            program,
            module: Some(module),
            function_ids: BTreeMap::new(),
            data_ids: BTreeMap::new(),
            allocator_id,
            text_builder_new_id,
            text_builder_append_id,
            text_builder_finish_id,
            f64_vec_new_id,
            text_concat_id,
            text_slice_id,
            text_from_f64_fixed_id,
            parse_i32_id,
            arg_count_id,
            arg_text_id,
            stdin_text_id,
            stdout_write_id,
            text_eq_id,
            records: collect_native_records(program).map_err(ObjectError::new)?,
            native_enums: collect_native_enums(program),
        })
    }

    fn emit(&mut self) -> Result<Vec<u8>, ObjectError> {
        self.declare_data_objects()?;
        self.define_data_objects()?;
        self.declare_functions()?;
        self.define_functions()?;
        let module = self.module.take().expect("module is available during emit");
        let result = module.finish();
        result
            .emit()
            .map_err(|error| ObjectError::new(format!("failed to emit object artifact: {error}")))
    }

    fn declare_data_objects(&mut self) -> Result<(), ObjectError> {
        let mut next_index = 0usize;
        let module = self.module.as_mut().expect("module available");
        for function in &self.program.functions {
            declare_text_data_for_insts(
                module,
                &mut self.data_ids,
                &function.instructions,
                "__sarif_text",
                &mut next_index,
                "object",
            )
            .map_err(ObjectError::new)?;
        }
        Ok(())
    }

    fn define_data_objects(&mut self) -> Result<(), ObjectError> {
        let module = self.module.as_mut().expect("module available");
        for (value, id) in &self.data_ids {
            let mut description = DataDescription::new();
            description.set_align(8);
            description.define(encode_text_blob(value).into_boxed_slice());
            module.define_data(*id, &description).map_err(|error| {
                ObjectError::new(format!(
                    "failed to define object text data for {value:?}: {error}"
                ))
            })?;
        }
        Ok(())
    }

    fn declare_functions(&mut self) -> Result<(), ObjectError> {
        for function in &self.program.functions {
            let symbol_name = if function.name == "main" {
                ENTRYPOINT_SYMBOL
            } else {
                &function.name
            };
            let signature = self.signature_for(function)?;
            let module = self.module.as_mut().expect("module available");
            let id = module
                .declare_function(symbol_name, Linkage::Export, &signature)
                .map_err(|error| {
                    ObjectError::new(format!(
                        "failed to declare `{symbol_name}` for object emission: {error}",
                    ))
                })?;
            self.function_ids.insert(function.name.clone(), id);
        }
        Ok(())
    }

    fn define_functions(&mut self) -> Result<(), ObjectError> {
        for function in &self.program.functions {
            let signature = self.signature_for(function)?;
            let mut context = self
                .module
                .as_ref()
                .expect("module available")
                .make_context();
            context.func.signature = signature;
            context.func.name = UserFuncName::user(0, self.function_ids[&function.name].as_u32());
            let mut builder_context = FunctionBuilderContext::new();

            // Lower function logic moved out of the module borrow scope
            self.lower_into_context(function, &mut context, &mut builder_context)?;

            let id = self.function_ids[&function.name];
            let module = self.module.as_mut().expect("module available");
            module.define_function(id, &mut context).map_err(|error| {
                ObjectError::new(format!(
                    "failed to define `{}` for object emission: {error}\n{}",
                    function.name,
                    context.func.display()
                ))
            })?;
        }
        Ok(())
    }

    fn signature_for(&self, function: &Function) -> Result<Signature, ObjectError> {
        let module = self.module.as_ref().expect("module available");
        let mut signature = module.make_signature();
        signature.call_conv = CallConv::triple_default(module.isa().triple());

        for param in &function.params {
            signature.params.push(AbiParam::new(native_type(
                &param.ty,
                &self.records,
                &self.native_enums,
            )?));
        }

        if let Some(return_type) = function.return_type.as_deref() {
            let native = native_type(return_type, &self.records, &self.native_enums)?;
            if native != types::INVALID {
                signature.returns.push(AbiParam::new(native));
            }
        }

        Ok(signature)
    }

    fn lower_into_context(
        &mut self,
        function: &Function,
        context: &mut cranelift_codegen::Context,
        builder_context: &mut FunctionBuilderContext,
    ) -> Result<(), ObjectError> {
        let mut builder = FunctionBuilder::new(&mut context.func, builder_context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let block_params = builder.block_params(entry).to_vec();
        let mut values = BTreeMap::<ValueId, ValueRepr>::new();
        let mut slot_vars = BTreeMap::<crate::LocalSlotId, Variable>::new();
        let value_kinds = infer_value_kinds(
            function,
            &self.records,
            &self.native_enums,
            &self.program.functions,
        )
        .map_err(ObjectError::new)?;
        for local in &function.mutable_locals {
            let var =
                builder.declare_var(native_type(&local.ty, &self.records, &self.native_enums)?);
            slot_vars.insert(local.slot, var);
        }

        let module = self.module.as_mut().expect("module available");
        let mut f64_vec_headers = BTreeMap::<cranelift_codegen::ir::Value, F64VecHeader>::new();
        let falls_through = lower_insts(
            &self.function_ids,
            &self.data_ids,
            self.allocator_id,
            self.text_builder_new_id,
            self.text_builder_append_id,
            self.text_builder_finish_id,
            self.f64_vec_new_id,
            self.text_concat_id,
            self.text_slice_id,
            self.text_from_f64_fixed_id,
            self.parse_i32_id,
            self.arg_count_id,
            self.arg_text_id,
            self.stdin_text_id,
            self.stdout_write_id,
            self.text_eq_id,
            &self.records,
            &self.native_enums,
            &value_kinds,
            module,
            function,
            &mut builder,
            &block_params,
            &slot_vars,
            &mut values,
            &mut f64_vec_headers,
            &TrustedF64VecAccesses::default(),
            &function.instructions,
            "object",
        )
        .map_err(ObjectError::new)?;

        if falls_through {
            let result = match function.result {
                Some(value) => value_repr(&values, value, function, "return")?,
                None => ValueRepr::Unit,
            };
            match result {
                ValueRepr::Native(value) => {
                    builder.ins().return_(&[value]);
                }
                ValueRepr::Unit => {
                    builder.ins().return_(&[]);
                }
            }
        }
        builder.finalize();
        Ok(())
    }
}

fn native_type(
    name: &str,
    records: &BTreeMap<String, NativeRecord>,
    enums: &BTreeMap<String, NativeEnum>,
) -> Result<types::Type, ObjectError> {
    shared_native_type(name, records, enums).map_err(ObjectError::new)
}

fn value_repr(
    values: &BTreeMap<ValueId, ValueRepr>,
    value: ValueId,
    function: &Function,
    context: &str,
) -> Result<ValueRepr, ObjectError> {
    shared_value_repr(values, value, function, context, "object").map_err(ObjectError::new)
}

#[cfg(test)]
mod tests {
    use sarif_frontend::hir::lower as lower_hir;
    use sarif_syntax::ast::lower as lower_ast;
    use sarif_syntax::lexer::lex;
    use sarif_syntax::parser::parse;

    use crate::emit_object;

    #[test]
    fn emits_object_for_integer_programs() {
        let lexed = lex(
            "fn add(left: I32, right: I32) -> I32 { left + right }\nfn main() -> I32 { add(20, 22) }",
        );
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        let mir = crate::lower(&hir.module);

        let bytes =
            emit_object(&mir.program, "sarif_add_test").expect("object emission should work");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn emits_object_for_text_programs() {
        let lexed = lex("fn main() -> Text { \"hello\" }");
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        let mir = crate::lower(&hir.module);

        let bytes =
            emit_object(&mir.program, "sarif_text_test").expect("object emission should work");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn emits_object_for_if_programs() {
        let lexed = lex("fn main() -> I32 { if true { 20 } else { 22 } }");
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        let mir = crate::lower(&hir.module);

        let bytes =
            emit_object(&mir.program, "sarif_if_test").expect("object emission should work");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn emits_object_for_record_programs() {
        let lexed = lex(
            "struct Pair { left: I32, right: I32 }\nfn main() -> I32 { Pair { left: 20, right: 22 }.left }",
        );
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        let mir = crate::lower(&hir.module);

        let bytes =
            emit_object(&mir.program, "sarif_record_test").expect("object emission should work");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn emits_object_for_nested_record_programs() {
        let lexed = lex(
            "struct Inner { value: I32 }\nstruct Outer { inner: Inner }\nfn main() -> I32 { Outer { inner: Inner { value: 42 } }.inner.value }",
        );
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        let mir = crate::lower(&hir.module);

        let bytes = emit_object(&mir.program, "sarif_nested_record_test")
            .expect("object emission should work");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn emits_object_for_payload_enum_programs() {
        let lexed = lex(
            "enum OptionText { none, some(Text) }\nfn main() -> Text { match OptionText.some(\"hello\") { OptionText.none => { \"none\" }, OptionText.some(text) => { text } } }",
        );
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        let mir = crate::lower(&hir.module);

        let bytes = emit_object(&mir.program, "sarif_payload_enum_test")
            .expect("object emission should work");
        assert!(!bytes.is_empty());
    }
}
