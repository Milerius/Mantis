//! x86_64 macro assembler types for structured assembly.
//!
//! Type definitions for the compile-time assembler DSL.
//! Maps from Constantine's `macro_assembler_x86_att.nim`.

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
    /// The RCX register specifically.
    RCX,
    /// The RDX register specifically.
    RDX,
    /// The R8 register specifically.
    R8,
    /// The RAX register specifically.
    RAX,
    /// x86 carry flag.
    CarryFlag,
    /// Register clobbered by the instruction.
    ClobberedReg,
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

/// x86_64 general-purpose registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register {
    /// Base register (callee-saved).
    Rbx,
    /// Data register / third argument.
    Rdx,
    /// Eighth register (extended).
    R8,
    /// Accumulator register.
    Rax,
    /// First SSE/AVX vector register.
    Xmm0,
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
    /// Element from an array at a given index.
    FromArray {
        /// Base register holding the array pointer.
        base: Register,
        /// Element index (not byte offset).
        offset: usize,
    },
}

// Note: AssemblerX86 struct (code buffer + operand tracking) requires
// String/Vec which need alloc. It will be added when the `alloc` feature
// is introduced. For now, only the zero-allocation enum types are defined.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rm_variants_are_distinct() {
        assert_ne!(RM::Reg, RM::Mem);
        assert_ne!(RM::Imm, RM::CarryFlag);
    }

    #[test]
    fn constraint_variants() {
        let c = Constraint::InputOutput;
        assert_eq!(c, Constraint::InputOutput);
        assert_ne!(c, Constraint::Input);
    }

    #[test]
    fn mem_indirect_access_variants() {
        assert_ne!(MemIndirectAccess::NoAccess, MemIndirectAccess::ReadWrite);
    }

    #[test]
    fn operand_construction() {
        let reg = Operand::Reg(Register::Rax);
        let imm = Operand::Imm(42);
        let mem = Operand::Mem { base: Register::Rbx, offset: 8 };
        let arr = Operand::FromArray { base: Register::Rdx, offset: 0 };

        assert_eq!(reg, Operand::Reg(Register::Rax));
        assert_eq!(imm, Operand::Imm(42));
        assert_eq!(mem, Operand::Mem { base: Register::Rbx, offset: 8 });
        assert_eq!(arr, Operand::FromArray { base: Register::Rdx, offset: 0 });
    }

    #[test]
    fn register_debug() {
        let s = core::format_args!("{:?}", Register::Rax);
        // Just verify it doesn't panic
        let _ = s;
    }
}
