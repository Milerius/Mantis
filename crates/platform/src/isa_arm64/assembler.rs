//! aarch64 macro assembler types for structured assembly.
//!
//! Type definitions for the compile-time assembler DSL.
//! Maps from Constantine's `macro_assembler_arm64.nim`.

/// Register or Memory operand kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RM {
    /// General-purpose register operand.
    Reg,
    /// Memory operand.
    Mem,
    /// Immediate operand.
    Imm,
    /// Memory operand with offsetting support.
    MemOffsettable,
    /// Pointer held in a register.
    PointerInReg,
    /// Array elements held in a register.
    ElemsInReg,
    /// ARM64 zero register.
    XZR,
    /// ARM carry flag (set on no-borrow for subtraction).
    CarryFlag,
    /// ARM borrow flag (inverted carry semantics).
    BorrowFlag,
    /// Register clobbered by the instruction.
    ClobberedReg,
}

/// ARM64 condition codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionCode {
    /// Equal / Zero.
    Eq,
    /// Not equal / Not zero.
    Ne,
    /// Carry set / Unsigned higher or same.
    Cs,
    /// Unsigned higher or same (alias for Cs).
    Hs,
    /// Carry clear / Unsigned lower.
    Cc,
    /// Unsigned lower (alias for Cc).
    Lo,
    /// Minus / Negative.
    Mi,
    /// Plus / Positive or zero.
    Pl,
    /// Overflow set.
    Vs,
    /// Overflow clear.
    Vc,
    /// Unsigned higher.
    Hi,
    /// Unsigned lower or same.
    Ls,
    /// Signed greater than or equal.
    Ge,
    /// Signed less than.
    Lt,
    /// Signed greater than.
    Gt,
    /// Signed less than or equal.
    Le,
    /// Always (unconditional).
    Al,
}

/// GCC extended assembly constraint modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constraint {
    /// Read-only input operand.
    Input,
    /// Commutative input operand.
    InputCommutative,
    /// Output operand that may overwrite input.
    OutputOverwrite,
    /// Output operand with early-clobber semantics.
    OutputEarlyClobber,
    /// Operand used as both input and output.
    InputOutput,
    /// Input/output operand with early-clobber semantics.
    InputOutputEarlyClobber,
    /// Register clobbered by the instruction.
    ClobberedRegister,
}

/// Memory indirect access mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemIndirectAccess {
    /// No memory access.
    NoAccess,
    /// Read-only memory access.
    Read,
    /// Write-only memory access.
    Write,
    /// Read-write memory access.
    ReadWrite,
}

/// aarch64 general-purpose registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register {
    /// Zero register / stack pointer (context-dependent).
    Xzr,
}

/// Operand kind discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// A single register operand.
    Register,
    /// An element drawn from an array in memory.
    FromArray,
    /// Address of the start of an array.
    ArrayAddr,
    /// Address within a 2D array.
    Array2dAddr,
}

/// Assembly operand.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// Register operand.
    Reg(Register),
    /// Immediate value.
    Imm(i64),
    /// Memory operand with base register and byte offset.
    Mem {
        /// Base register for the memory address.
        base: Register,
        /// Byte offset from the base register.
        offset: i32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rm_arm64_specific_variants() {
        assert_ne!(RM::XZR, RM::CarryFlag);
        assert_ne!(RM::CarryFlag, RM::BorrowFlag);
    }

    #[test]
    fn condition_code_all_variants() {
        let codes = [
            ConditionCode::Eq, ConditionCode::Ne,
            ConditionCode::Cs, ConditionCode::Hs,
            ConditionCode::Cc, ConditionCode::Lo,
            ConditionCode::Mi, ConditionCode::Pl,
            ConditionCode::Vs, ConditionCode::Vc,
            ConditionCode::Hi, ConditionCode::Ls,
            ConditionCode::Ge, ConditionCode::Lt,
            ConditionCode::Gt, ConditionCode::Le,
            ConditionCode::Al,
        ];
        assert_eq!(codes.len(), 17);
    }

    #[test]
    fn operand_construction() {
        let reg = Operand::Reg(Register::Xzr);
        let imm = Operand::Imm(-1);
        let mem = Operand::Mem { base: Register::Xzr, offset: 16 };
        assert_eq!(reg, Operand::Reg(Register::Xzr));
        assert_eq!(imm, Operand::Imm(-1));
        assert_eq!(mem, Operand::Mem { base: Register::Xzr, offset: 16 });
    }
}
