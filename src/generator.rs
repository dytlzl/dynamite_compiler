use crate::node::{Node, NodeType};
use crate::error::{error, error_at};
use crate::ast::{ASTBuilder};
use crate::ctype::Type;
use crate::func::Func;
use crate::instruction::{Instruction, InstOperator, InstOperand, Register, Assembly};
use crate::instruction::{InstOperator::*, Register::*};
use std::fmt::Display;
use crate::global::{GlobalVariable, GlobalVariableData};
use crate::instruction::InstOperand::{Ptr, PtrAdd, ElseFlag, EndFlag, BeginFlag};

pub struct AsmGenerator<'a> {
    code: &'a str,
    target_os: Os,
    branch_count: usize,
    loop_stack: Vec<usize>,
    builder: &'a ASTBuilder<'a>,
    current_stack_size: usize,
    pub assemblies: Vec<Assembly>,
}

#[derive(Clone, Copy)]
pub enum Os {
    Linux,
    MacOS,
}

const ARGS_REG: [Register; 6] = [RDI, RSI, RDX, RCX, R8, R9];

impl<'a> AsmGenerator<'a> {
    pub fn new(builder: &'a ASTBuilder<'a>, code: &'a str, target_os: Os) -> Self {
        Self {
            code,
            target_os,
            builder,
            branch_count: 0,
            loop_stack: Vec::new(),
            current_stack_size: 0,
            assemblies: Vec::new(),
        }
    }

    pub fn gen(&mut self) {
        self.push_assembly(".intel_syntax noprefix");
        if let Os::MacOS = self.target_os {
            self.push_assembly(".section __TEXT,__text,regular,pure_instructions");
        } else {
            self.push_assembly(".section .text");
        }
        for (s, f) in &self.builder.functions {
            self.gen_func(s, f);
        }
        if let Os::MacOS = self.target_os {
            self.push_assembly(".section __DATA,__data");
        } else {
            self.push_assembly(".section .data");
        }
        for (s, gv) in &self.builder.global_variables {
            self.gen_global_variable(s, gv);
        }
        self.gen_string_literals();
    }

    pub fn gen_string_literals(&mut self) {
        if self.builder.string_literals.len() != 0 {
            if let Os::MacOS = self.target_os {
                self.push_assembly(".section __TEXT,__cstring,cstring_literals");
            } else {
                self.push_assembly(".section .data");
            }
            for (i, str) in self.builder.string_literals.iter().enumerate() {
                self.push_assembly(format!("L_.str.{}:", i));
                self.push_assembly(format!("  .asciz \"{}\"", str));
            }
        }
    }

    pub fn gen_global_variable(&mut self, name: &str, gv: &GlobalVariable) {
        self.push_assembly(format!("{}:", self.with_prefix(name)));
        self.gen_initializer_element(&gv.ty, gv.data.as_ref())
    }

    pub fn gen_initializer_element(&mut self, ty: &Type, data: Option<&GlobalVariableData>) {
        match ty {
            Type::Arr(children_ty, size) => {
                let mut rest_count = *size;
                if let Some(GlobalVariableData::Arr(v)) = data {
                    for (i, d) in v.iter().enumerate() {
                        if i >= *size {
                            break;
                        }
                        self.gen_initializer_element(children_ty.as_ref(), Some(d));
                    }
                    rest_count -= v.len();
                }
                self.push_assembly(format!("  .zero {}", children_ty.size_of() * rest_count));
            }
            Type::I8 => {
                self.push_assembly(format!(
                    "  .byte {}",
                    if let Some(GlobalVariableData::Elem(s)) = data { s } else { "0" }
                ));
            }
            Type::I32 => {
                self.push_assembly(format!(
                    "  .4byte {}",
                    if let Some(GlobalVariableData::Elem(s)) = data { s } else { "0" }
                ));
            }
            _ => {
                self.push_assembly(format!(
                    "  .8byte {}",
                    if let Some(GlobalVariableData::Elem(s)) = data { s } else { "0" }
                ));
            }
        }
    }

    pub fn gen_func(&mut self, name: &str, func: &Func) {
        if let None = func.body {
            return;
        }
        self.push_assembly(format!(".globl {}", self.with_prefix(name)));
        self.push_assembly(format!("{}:", self.with_prefix(name)));

        // prologue
        self.inst1(PUSH, RBP);
        self.inst2(MOV, RBP, RSP);
        self.inst2(SUB, RSP, func.offset_size);
        for (i, arg) in func.args.iter().enumerate() {
            if let NodeType::LocalVar = arg.nt {
                self.inst2(MOV, RAX, RBP);
                self.inst2(SUB, RAX, arg.offset.unwrap());
                self.inst2(MOV, Ptr(RAX, 8), ARGS_REG[i]);
            } else {
                error_at(self.code, arg.token.as_ref().unwrap().pos, "ident expected");
            }
        }
        self.current_stack_size = func.offset_size;
        self.gen_with_node(func.body.as_ref().unwrap());
        self.inst2(MOV, RAX, 0); // default return value
        self.epilogue();
    }

    fn epilogue(&mut self) {
        self.inst2(MOV, RSP, RBP);
        self.inst1(POP, RBP);
        self.inst0(RET);
    }

    fn gen_with_node(&mut self, node: &Node) {
        match node.nt {
            NodeType::DefVar => {
                self.gen_with_vec(&node.children);
                return;
            }
            NodeType::CallFunc => {
                self.inst2(MOV, RAX, RSP);
                self.inst2(ADD, RAX, 8);
                self.inst2(MOV, RDI, 16);
                self.inst0(CQO);
                self.inst1(IDIV, RDI);
                self.inst2(SUB, RSP, RDX);
                self.inst1(PUSH, RDX);
                for node in &node.args {
                    self.gen_with_node(node);
                }
                for i in 0..node.args.len() {
                    self.inst1(POP, ARGS_REG[i]);
                }
                self.inst1(CALL, self.with_prefix(&node.global_name));
                self.inst1(POP, RDI);
                self.inst2(ADD, RSP, RDI);
                self.inst1(PUSH, RAX);
                return;
            }
            NodeType::If => {
                let branch_num = self.new_branch_num();
                self.gen_with_node(node.cond.as_ref().unwrap());
                self.inst1(POP, RAX);
                self.inst2(CMP, RAX, 0);
                self.inst1(JE, ElseFlag(branch_num));
                self.gen_with_node(node.then.as_ref().unwrap());
                self.inst1(JMP, EndFlag(branch_num));
                self.push_assembly(format!("{}:", ElseFlag(branch_num)));
                if let Some(_) = node.els {
                    self.gen_with_node(node.els.as_ref().unwrap());
                }
                self.push_assembly(format!("{}:", EndFlag(branch_num)));
                return;
            }
            NodeType::While => {
                let branch_num = self.new_branch_num();
                self.loop_stack.push(branch_num);
                self.push_assembly(format!("{}:", BeginFlag(branch_num)));
                self.reset_stack();
                self.gen_with_node(node.cond.as_ref().unwrap());
                self.inst1(POP, RAX);
                self.inst2(CMP, RAX, 0);
                self.inst1(JE, EndFlag(branch_num));
                self.gen_with_node(node.then.as_ref().unwrap());
                self.inst1(JMP, BeginFlag(branch_num));
                self.push_assembly(format!("{}:", EndFlag(branch_num)));
                self.loop_stack.pop();
                return;
            }
            NodeType::For => {
                let branch_num = self.new_branch_num();
                self.loop_stack.push(branch_num);
                if let Some(_) = node.ini {
                    self.gen_with_node(node.ini.as_ref().unwrap());
                    self.inst1(POP, RAX);
                }
                self.push_assembly(format!("{}:", BeginFlag(branch_num)));
                self.reset_stack();
                if let Some(_) = node.cond {
                    self.gen_with_node(node.cond.as_ref().unwrap());
                    self.inst1(POP, RAX);
                } else {
                    self.inst2(MOV, RAX, 1);
                }
                self.inst2(CMP, RAX, 0);
                self.inst1(JE, EndFlag(branch_num));
                self.gen_with_node(node.then.as_ref().unwrap());
                if let Some(_) = node.upd {
                    self.gen_with_node(node.upd.as_ref().unwrap());
                    self.inst1(POP, RAX);
                }
                self.inst1(JMP, BeginFlag(branch_num));
                self.push_assembly(format!("{}:", EndFlag(branch_num)));
                self.loop_stack.pop();
                return;
            }
            NodeType::Block => {
                self.gen_with_vec(&node.children);
                return;
            }
            NodeType::Break => {
                if let Some(&branch_num) = self.loop_stack.last() {
                    self.inst1(JMP, EndFlag(branch_num.clone()));
                } else {
                    error_at(self.code, node.token.as_ref().unwrap().pos, "unexpected break found");
                }
                return;
            }
            NodeType::Return => {
                self.gen_with_node(node.lhs.as_ref().unwrap());
                self.inst1(POP, RAX);
                self.epilogue();
                return;
            }
            NodeType::Num => {
                self.inst1(PUSH, node.value.unwrap());
                return;
            }
            NodeType::LocalVar | NodeType::GlobalVar => {
                self.gen_addr(node);
                if let Some(Type::Arr(_, _)) = node.resolve_type() {
                    return;
                }
                self.inst1(POP, RAX);
                self.deref_rax(node);
                self.inst1(PUSH, RAX);
                return;
            }
            NodeType::Addr => {
                self.gen_addr(node.lhs.as_ref().unwrap());
                return;
            }
            NodeType::Deref => {
                self.gen_with_node(node.lhs.as_ref().unwrap());
                if let Some(Type::Arr(..)) = node.lhs.as_ref().unwrap().dest_type() {
                    return;
                }
                self.inst1(POP, RAX);
                self.deref_rax(node);
                self.inst1(PUSH, RAX);
                return;
            }
            NodeType::Assign => {
                self.gen_addr(node.lhs.as_ref().unwrap());
                self.gen_with_node(node.rhs.as_ref().unwrap());
                self.inst1(POP, RDI);
                self.inst1(POP, RAX);
                self.operation2rdi(node.lhs.as_ref().unwrap().resolve_type(), MOV, RAX);
                self.inst1(PUSH, RDI);
                return;
            }
            NodeType::BitLeft | NodeType::BitRight => {
                self.gen_with_node(node.rhs.as_ref().unwrap());
                self.gen_with_node(node.lhs.as_ref().unwrap());
                self.inst1(POP, RAX);
                self.inst1(POP, RCX);
                self.inst2(match node.nt {
                    NodeType::BitLeft => SHL,
                    NodeType::BitRight => SAR,
                    _ => { unreachable!() }
                }, RAX, CL);
                self.inst1(PUSH, RAX);
                return;
            }
            NodeType::BitNot => {
                self.gen_with_node(node.lhs.as_ref().unwrap());
                self.inst1(POP, RAX);
                self.inst1(NOT, RAX);
                self.inst1(PUSH, RAX);
                return;
            }
            NodeType::LogicalAnd => {
                let branch_num = self.new_branch_num();
                self.gen_with_node(node.lhs.as_ref().unwrap());
                self.inst1(POP, RAX);
                self.inst2(CMP, RAX, 0);
                self.inst1(JE, EndFlag(branch_num));
                self.gen_with_node(node.rhs.as_ref().unwrap());
                self.inst1(POP, RAX);
                self.push_assembly(format!("{}:", EndFlag(branch_num)));
                self.inst1(PUSH, RAX);
                return;
            }
            NodeType::LogicalOr => {
                let branch_num = self.new_branch_num();
                self.gen_with_node(node.lhs.as_ref().unwrap());
                self.inst1(POP, RAX);
                self.inst2(CMP, RAX, 0);
                self.inst1(JNE, EndFlag(branch_num));
                self.gen_with_node(node.rhs.as_ref().unwrap());
                self.inst1(POP, RAX);
                self.push_assembly(format!("{}:", EndFlag(branch_num)));
                self.inst1(PUSH, RAX);
                return;
            }
            NodeType::SuffixIncr | NodeType::SuffixDecr => {
                self.gen_addr(node.lhs.as_ref().unwrap());
                self.inst1(POP, RAX);
                self.inst2(MOV, RDI, 1);
                if let Some(t) = node.lhs.as_ref().unwrap().dest_type() {
                    self.inst2(IMUL, RDI, t.size_of());
                }
                self.inst2(MOV, RDX, RAX);
                self.deref_rax(node.lhs.as_ref().unwrap());
                let op = if let NodeType::SuffixIncr = node.nt { ADD } else { SUB };
                self.operation2rdi(node.lhs.as_ref().unwrap().resolve_type(), op, RDX);
                self.inst1(PUSH, RAX);
                return;
            }
            _ => {}
        }
        self.gen_with_node(node.rhs.as_ref().unwrap());
        self.gen_with_node(node.lhs.as_ref().unwrap());
        self.inst1(POP, RAX);
        self.inst1(POP, RDI);
        match node.nt {
            NodeType::Add => {
                if let Some(t) = node.lhs.as_ref().unwrap().dest_type() {
                    self.inst2(IMUL, RDI, t.size_of());
                }
                self.inst2(ADD, RAX, RDI);
            }
            NodeType::Sub => {
                if let Some(t) = node.lhs.as_ref().unwrap().dest_type() {
                    self.inst2(IMUL, RDI, t.size_of());
                }
                self.inst2(SUB, RAX, RDI);
            }
            NodeType::Mul => {
                self.inst2(IMUL, RAX, RDI);
            }
            NodeType::Div => {
                self.inst0(CQO);
                self.inst1(IDIV, RDI);
            }
            NodeType::Mod => {
                self.inst0(CQO);
                self.inst1(IDIV, RDI);
                self.inst2(MOV, RAX, RDX);
            }
            NodeType::Eq | NodeType::Ne | NodeType::Lt | NodeType::Le => {
                self.inst2(CMP, RAX, RDI);
                self.inst1(match node.nt {
                    NodeType::Eq => SETE,
                    NodeType::Ne => SETNE,
                    NodeType::Lt => SETL,
                    NodeType::Le => SETLE,
                    _ => unreachable!()
                }, AL);
                self.inst2(MOVZX, RAX, AL);
            }
            NodeType::BitAnd => {
                self.inst2(AND, RAX, RDI);
            }
            NodeType::BitXor => {
                self.inst2(XOR, RAX, RDI);
            }
            NodeType::BitOr => {
                self.inst2(OR, RAX, RDI);
            }
            _ => {
                error("unexpected node");
            }
        }
        self.inst1(PUSH, RAX);
    }

    fn gen_with_vec(&mut self, v: &Vec<Node>) {
        for node in v {
            self.gen_with_node(node);
            self.inst1(POP, RAX);
            self.reset_stack();
        }
    }

    fn gen_addr(&mut self, node: &Node) {
        match node.nt {
            NodeType::GlobalVar => {
                if node.dest != "" {
                    self.inst2(LEA, RAX, PtrAdd(RIP, node.dest.clone()));
                } else {
                    self.inst2(LEA, RAX, PtrAdd(RIP, self.with_prefix(&node.global_name)));
                }
                self.inst1(PUSH, RAX);
            }
            NodeType::LocalVar => {
                self.inst2(MOV, RAX, RBP);
                self.inst2(SUB, RAX, node.offset.unwrap());
                self.inst1(PUSH, RAX);
            }
            NodeType::Deref => {
                self.gen_with_node(node.lhs.as_ref().unwrap());
            }
            _ => {
                unreachable!();
            }
        }
    }

    fn operation2rdi(&mut self, c_type: Option<Type>, operator: InstOperator, from: Register) {
        match c_type {
            Some(Type::I8) => {
                self.inst2(operator, Ptr(from, 1), DIL);
            }
            Some(Type::I32) => {
                self.inst2(operator, Ptr(from, 4), EDI);
            }
            _ => {
                self.inst2(operator, Ptr(from, 8), RDI);
            }
        }
    }

    fn deref_rax(&mut self, node: &Node) {
        match node.resolve_type() {
            Some(Type::I32) => {
                self.inst2(MOVSXD, RAX, Ptr(RAX, 4));
            }
            Some(Type::I8) => {
                self.inst2(MOVSX, RAX, Ptr(RAX, 1));
            }
            _ => {
                self.inst2(MOV, RAX, Ptr(RAX, 8));
            }
        }
    }

    fn inst0(&mut self, operator: InstOperator) {
        self.assemblies.push(
            Assembly::Inst(
                Instruction { operator, operand1: None, operand2: None }
            )
        )
    }
    fn inst1<T1>(&mut self, operator: InstOperator, operand1: T1) where
        T1: Into<InstOperand> {
        self.assemblies.push(
            Assembly::Inst(
                Instruction { operator, operand1: Some(operand1.into()), operand2: None }
            )
        )
    }
    fn inst2<T1, T2>(&mut self, operator: InstOperator, operand1: T1, operand2: T2) where
        T1: Into<InstOperand>, T2: Into<InstOperand> {
        self.assemblies.push(
            Assembly::Inst(
                Instruction { operator, operand1: Some(operand1.into()), operand2: Some(operand2.into()) }
            )
        )
    }

    fn push_assembly(&mut self, s: impl ToString) {
        self.assemblies.push(Assembly::Other(s.to_string()))
    }

    fn with_prefix<T: Display>(&self, s: T) -> String {
        format!("{}{}", if let Os::MacOS = self.target_os { "_" } else { "" }, s)
    }

    fn new_branch_num(&mut self) -> usize {
        self.branch_count += 1;
        self.branch_count
    }

    fn reset_stack(&mut self) {
        self.inst2(MOV, RSP, RBP);
        self.inst2(SUB, RSP, self.current_stack_size);
    }
}