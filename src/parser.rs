//! Functions for parsing DWARF debugging information.

use std::error;
use std::ffi;
use std::fmt::{self, Debug};
use std::io;
use std::result;
use cfi::BaseAddresses;
use constants;
use endianity::{Endianity, EndianBuf};
use leb128;

/// An error that occurred when parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Found a CFI relative pointer, but the CFI base is undefined.
    CfiRelativePointerButCfiBaseIsUndefined,
    /// Found a `.text` relative pointer, but the `.text` base is undefined.
    TextRelativePointerButTextBaseIsUndefined,
    /// Found a `.data` relative pointer, but the `.data` base is undefined.
    DataRelativePointerButDataBaseIsUndefined,
    /// Found a function relative pointer in a context that does not have a
    /// function base.
    FuncRelativePointerInBadContext,
    /// An error parsing an unsigned LEB128 value.
    BadUnsignedLeb128,
    /// An error parsing a signed LEB128 value.
    BadSignedLeb128,
    /// An abbreviation declared that its tag is zero, but zero is reserved for
    /// null records.
    AbbreviationTagZero,
    /// An attribute specification declared that its form is zero, but zero is
    /// reserved for null records.
    AttributeFormZero,
    /// The abbreviation's has-children byte was not one of
    /// `DW_CHILDREN_{yes,no}`.
    BadHasChildren,
    /// The specified length is impossible.
    BadLength,
    /// Found an unknown `DW_FORM_*` type.
    UnknownForm,
    /// Expected a zero, found something else.
    ExpectedZero,
    /// Found an abbreviation code that has already been used.
    DuplicateAbbreviationCode,
    /// Found a duplicate arange.
    DuplicateArange,
    /// Found an unknown reserved length value.
    UnknownReservedLength,
    /// Found an unknown DWARF version.
    UnknownVersion,
    /// The unit header's claimed length is too short to even hold the header
    /// itself.
    UnitHeaderLengthTooShort,
    /// Found a record with an unknown abbreviation code.
    UnknownAbbreviation,
    /// Hit the end of input before it was expected.
    UnexpectedEof,
    /// Read a null entry before it was expected.
    UnexpectedNull,
    /// Found an unknown standard opcode.
    UnknownStandardOpcode(constants::DwLns),
    /// Found an unknown extended opcode.
    UnknownExtendedOpcode(constants::DwLne),
    /// The specified address size is not supported.
    UnsupportedAddressSize(u8),
    /// The specified field size is not supported.
    UnsupportedFieldSize(u8),
    /// The minimum instruction length must not be zero.
    MinimumInstructionLengthZero,
    /// The maximum operations per instruction must not be zero.
    MaximumOperationsPerInstructionZero,
    /// The line range must not be zero.
    LineRangeZero,
    /// The opcode base must not be zero.
    OpcodeBaseZero,
    /// Found an invalid UTF-8 string.
    BadUtf8,
    /// Expected to find the CIE ID, but found something else.
    NotCieId,
    /// Expected to find a pointer to a CIE, but found the CIE ID instead.
    NotCiePointer,
    /// Invalid branch target for a DW_OP_bra or DW_OP_skip.
    BadBranchTarget(usize),
    /// DW_OP_push_object_address used but no address passed in.
    InvalidPushObjectAddress,
    /// Not enough items on the stack when evaluating an expression.
    NotEnoughStackItems,
    /// Too many iterations to compute the expression.
    TooManyIterations,
    /// An unrecognized operation was found while parsing a DWARF
    /// expression.
    InvalidExpression(constants::DwOp),
    /// The expression had a piece followed by an expression
    /// terminator without a piece.
    InvalidPiece,
    /// An expression-terminating operation was followed by something
    /// other than the end of the expression or a piece operation.
    InvalidExpressionTerminator(usize),
    /// Division or modulus by zero when evaluating an expression.
    DivisionByZero,
    /// An unknown DW_CFA_* instruction.
    UnknownCallFrameInstruction(constants::DwCfa),
    /// The end of an address range was before the beginning.
    InvalidAddressRange,
    /// The end offset of a loc list entry was before the beginning.
    InvalidLocationAddressRange,
    /// Encountered a call frame instruction in a context in which it is not
    /// valid.
    CfiInstructionInInvalidContext,
    /// When evaluating call frame instructions, found a `DW_CFA_restore_state`
    /// stack pop instruction, but the stack was empty, and had nothing to pop.
    PopWithEmptyStack,
    /// Do not have unwind info for the given address.
    NoUnwindInfoForAddress,
    /// An offset value was larger than the maximum supported value.
    UnsupportedOffset,
    /// The given pointer encoding is either unknown or invalid.
    UnknownPointerEncoding,
    /// Did not find an entry at the given offset.
    NoEntryAtGivenOffset,
    /// The given offset is out of bounds.
    OffsetOutOfBounds,
    /// Found an unknown CFI augmentation.
    UnknownAugmentation,
    /// We do not support the given pointer encoding yet.
    UnsupportedPointerEncoding,
    /// We tried to convert some number into a `u8`, but it was too large.
    CannotFitInU8,
    /// The CFI program defined more register rules than we have storage for.
    TooManyRegisterRules,
    /// Attempted to push onto the CFI stack, but it was already at full
    /// capacity.
    CfiStackFull,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> ::std::result::Result<(), fmt::Error> {
        Debug::fmt(self, f)
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::CfiRelativePointerButCfiBaseIsUndefined => {
                "Found a CFI relative pointer, but the CFI base is undefined."
            }
            Error::TextRelativePointerButTextBaseIsUndefined => {
                "Found a `.text` relative pointer, but the `.text` base is undefined."
            }
            Error::DataRelativePointerButDataBaseIsUndefined => {
                "Found a `.data` relative pointer, but the `.data` base is undefined."
            }
            Error::FuncRelativePointerInBadContext => {
                "Found a function relative pointer in a context that does not have a function base."
            }
            Error::BadUnsignedLeb128 => "An error parsing an unsigned LEB128 value",
            Error::BadSignedLeb128 => "An error parsing a signed LEB128 value",
            Error::AbbreviationTagZero => {
                "An abbreviation declared that its tag is zero,
                 but zero is reserved for null records"
            }
            Error::AttributeFormZero => {
                "An attribute specification declared that its form is zero,
                 but zero is reserved for null records"
            }
            Error::BadHasChildren => {
                "The abbreviation's has-children byte was not one of
                 `DW_CHILDREN_{yes,no}`"
            }
            Error::BadLength => "The specified length is impossible",
            Error::UnknownForm => "Found an unknown `DW_FORM_*` type",
            Error::ExpectedZero => "Expected a zero, found something else",
            Error::DuplicateAbbreviationCode => {
                "Found an abbreviation code that has already been used"
            }
            Error::DuplicateArange => "Found a duplicate arange",
            Error::UnknownReservedLength => "Found an unknown reserved length value",
            Error::UnknownVersion => "Found an unknown DWARF version",
            Error::UnitHeaderLengthTooShort => {
                "The unit header's claimed length is too short to even hold
                 the header itself"
            }
            Error::UnknownAbbreviation => "Found a record with an unknown abbreviation code",
            Error::UnexpectedEof => "Hit the end of input before it was expected",
            Error::UnexpectedNull => "Read a null entry before it was expected.",
            Error::UnknownStandardOpcode(_) => "Found an unknown standard opcode",
            Error::UnknownExtendedOpcode(_) => "Found an unknown extended opcode",
            Error::UnsupportedAddressSize(_) => "The specified address size is not supported",
            Error::UnsupportedFieldSize(_) => "The specified field size is not supported",
            Error::MinimumInstructionLengthZero => {
                "The minimum instruction length must not be zero."
            }
            Error::MaximumOperationsPerInstructionZero => {
                "The maximum operations per instruction must not be zero."
            }
            Error::LineRangeZero => "The line range must not be zero.",
            Error::OpcodeBaseZero => "The opcode base must not be zero.",
            Error::BadUtf8 => "Found an invalid UTF-8 string.",
            Error::NotCieId => "Expected to find the CIE ID, but found something else.",
            Error::NotCiePointer => "Expected to find a CIE pointer, but found the CIE ID instead.",
            Error::BadBranchTarget(_) => "Invalid branch target in DWARF expression",
            Error::InvalidPushObjectAddress => {
                "DW_OP_push_object_address used but no object address given"
            }
            Error::NotEnoughStackItems => "Not enough items on stack when evaluating expression",
            Error::TooManyIterations => "Too many iterations to evaluate DWARF expression",
            Error::InvalidExpression(_) => "Invalid opcode in DWARF expression",
            Error::InvalidPiece => {
                "DWARF expression has piece followed by non-piece expression at end"
            }
            Error::InvalidExpressionTerminator(_) => "Expected DW_OP_piece or DW_OP_bit_piece",
            Error::DivisionByZero => "Division or modulus by zero when evaluating expression",
            Error::UnknownCallFrameInstruction(_) => "An unknown DW_CFA_* instructiion",
            Error::InvalidAddressRange => {
                "The end of an address range must not be before the beginning."
            }
            Error::InvalidLocationAddressRange => {
                "The end offset of a location list entry must not be before the beginning."
            }
            Error::CfiInstructionInInvalidContext => {
                "Encountered a call frame instruction in a context in which it is not valid."
            }
            Error::PopWithEmptyStack => {
                "When evaluating call frame instructions, found a `DW_CFA_restore_state` stack pop \
                 instruction, but the stack was empty, and had nothing to pop."
            }
            Error::NoUnwindInfoForAddress => "Do not have unwind info for the given address.",
            Error::UnsupportedOffset => {
                "An offset value was larger than the maximum supported value."
            }
            Error::UnknownPointerEncoding => {
                "The given pointer encoding is either unknown or invalid."
            }
            Error::NoEntryAtGivenOffset => "Did not find an entry at the given offset.",
            Error::OffsetOutOfBounds => "The given offset is out of bounds.",
            Error::UnknownAugmentation => "Found an unknown CFI augmentation.",
            Error::UnsupportedPointerEncoding => {
                "We do not support the given pointer encoding yet."
            }
            Error::CannotFitInU8 => {
                "We tried to convert some number into a `u8`, but it was too large."
            }
            Error::TooManyRegisterRules => {
                "The CFI program defined more register rules than we have storage for."
            }
            Error::CfiStackFull => {
                "Attempted to push onto the CFI stack, but it was already at full capacity."
            }
        }
    }
}

/// The result of a parse.
pub type Result<T> = result::Result<T, Error>;

/// Parse a `u8` from the input.
#[doc(hidden)]
#[inline]
pub fn parse_u8<Endian>(input: &mut EndianBuf<Endian>) -> Result<u8>
    where Endian: Endianity
{
    if input.is_empty() {
        Err(Error::UnexpectedEof)
    } else {
        let val = input[0];
        *input = input.range_from(1..);
        Ok(val)
    }
}

/// Parse a `i8` from the input.
#[doc(hidden)]
#[inline]
pub fn parse_i8<Endian>(input: &mut EndianBuf<Endian>) -> Result<i8>
    where Endian: Endianity
{
    if input.is_empty() {
        Err(Error::UnexpectedEof)
    } else {
        let val = input[0] as i8;
        *input = input.range_from(1..);
        Ok(val)
    }
}

/// Parse a `u16` from the input.
#[doc(hidden)]
#[inline]
pub fn parse_u16<Endian>(input: &mut EndianBuf<Endian>) -> Result<u16>
    where Endian: Endianity
{
    if input.len() < 2 {
        Err(Error::UnexpectedEof)
    } else {
        let val = Endian::read_u16(&input);
        *input = input.range_from(2..);
        Ok(val)
    }
}

/// Parse a `i16` from the input.
#[doc(hidden)]
#[inline]
pub fn parse_i16<Endian>(input: &mut EndianBuf<Endian>) -> Result<i16>
    where Endian: Endianity
{
    if input.len() < 2 {
        Err(Error::UnexpectedEof)
    } else {
        let val = Endian::read_i16(&input);
        *input = input.range_from(2..);
        Ok(val)
    }
}

/// Parse a `u32` from the input.
#[doc(hidden)]
#[inline]
pub fn parse_u32<Endian>(input: &mut EndianBuf<Endian>) -> Result<u32>
    where Endian: Endianity
{
    if input.len() < 4 {
        Err(Error::UnexpectedEof)
    } else {
        let val = Endian::read_u32(&input);
        *input = input.range_from(4..);
        Ok(val)
    }
}

/// Parse a `i32` from the input.
#[doc(hidden)]
#[inline]
pub fn parse_i32<Endian>(input: &mut EndianBuf<Endian>) -> Result<i32>
    where Endian: Endianity
{
    if input.len() < 4 {
        Err(Error::UnexpectedEof)
    } else {
        let val = Endian::read_i32(&input);
        *input = input.range_from(4..);
        Ok(val)
    }
}

/// Parse a `u64` from the input.
#[doc(hidden)]
#[inline]
pub fn parse_u64<Endian>(input: &mut EndianBuf<Endian>) -> Result<u64>
    where Endian: Endianity
{
    if input.len() < 8 {
        Err(Error::UnexpectedEof)
    } else {
        let val = Endian::read_u64(&input);
        *input = input.range_from(8..);
        Ok(val)
    }
}

/// Parse a `i64` from the input.
#[doc(hidden)]
#[inline]
pub fn parse_i64<Endian>(input: &mut EndianBuf<Endian>) -> Result<i64>
    where Endian: Endianity
{
    if input.len() < 8 {
        Err(Error::UnexpectedEof)
    } else {
        let val = Endian::read_i64(&input);
        *input = input.range_from(8..);
        Ok(val)
    }
}

/// Parse an unsigned LEB128 encoded integer.
#[doc(hidden)]
#[inline]
pub fn parse_unsigned_leb<Endian>(input: &mut EndianBuf<Endian>) -> Result<u64>
    where Endian: Endianity
{
    match leb128::read::unsigned(input) {
        Ok(val) => Ok(val),
        Err(leb128::read::Error::IoError(ref e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
            Err(Error::UnexpectedEof)
        }
        Err(_) => Err(Error::BadUnsignedLeb128),
    }
}

/// Parse an unsigned LEB128 encoded integer return it as a `u8`.
#[doc(hidden)]
#[inline]
pub fn parse_unsigned_leb_as_u8<Endian>(input: &mut EndianBuf<Endian>) -> Result<u8>
    where Endian: Endianity
{
    parse_unsigned_leb(input).and_then(u64_to_u8)
}

/// Parse a signed LEB128 encoded integer.
#[doc(hidden)]
#[inline]
pub fn parse_signed_leb<Endian>(input: &mut EndianBuf<Endian>) -> Result<i64>
    where Endian: Endianity
{
    match leb128::read::signed(input) {
        Ok(val) => Ok(val),
        Err(leb128::read::Error::IoError(ref e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
            Err(Error::UnexpectedEof)
        }
        Err(_) => Err(Error::BadSignedLeb128),
    }
}

/// Parse a `u32` from the input and return it as a `u64`.
#[doc(hidden)]
#[inline]
pub fn parse_u32_as_u64<Endian>(input: &mut EndianBuf<Endian>) -> Result<u64>
    where Endian: Endianity
{
    parse_u32(input).map(|v| v as u64)
}

/// Convert a `u64` to a `usize` and return it.
#[doc(hidden)]
#[inline]
pub fn u64_to_offset(offset64: u64) -> Result<usize> {
    let offset = offset64 as usize;
    if offset as u64 == offset64 {
        Ok(offset)
    } else {
        Err(Error::UnsupportedOffset)
    }
}

/// Convert a `u64` to a `u8` and return it.
#[doc(hidden)]
#[inline]
pub fn u64_to_u8(x: u64) -> Result<u8> {
    let y = x as u8;
    if y as u64 == x {
        Ok(y)
    } else {
        Err(Error::CannotFitInU8)
    }
}

/// Parse a `u64` from the input, and return it as a `usize`.
#[doc(hidden)]
#[inline]
pub fn parse_u64_as_offset<Endian>(input: &mut EndianBuf<Endian>) -> Result<usize>
    where Endian: Endianity
{
    parse_u64(input).and_then(u64_to_offset)
}

/// Parse an unsigned LEB128 encoded integer from the input, and return it as a `usize`.
#[doc(hidden)]
#[inline]
pub fn parse_uleb_as_offset<Endian>(input: &mut EndianBuf<Endian>) -> Result<usize>
    where Endian: Endianity
{
    parse_unsigned_leb(input).and_then(u64_to_offset)
}

/// Parse a word-sized integer according to the DWARF format, and return it as a `u64`.
#[doc(hidden)]
#[inline]
pub fn parse_word<Endian>(input: &mut EndianBuf<Endian>, format: Format) -> Result<u64>
    where Endian: Endianity
{
    match format {
        Format::Dwarf32 => parse_u32_as_u64(input),
        Format::Dwarf64 => parse_u64(input),
    }
}

/// Parse a word-sized integer according to the DWARF format, and return it as a `usize`.
#[doc(hidden)]
#[inline]
pub fn parse_offset<Endian>(input: &mut EndianBuf<Endian>, format: Format) -> Result<usize>
    where Endian: Endianity
{
    parse_word(input, format).and_then(u64_to_offset)
}

/// Parse an address-sized integer, and return it as a `u64`.
#[doc(hidden)]
#[inline]
pub fn parse_address<Endian>(input: &mut EndianBuf<Endian>, address_size: u8) -> Result<u64>
    where Endian: Endianity
{
    match address_size {
        8 => parse_u64(input),
        4 => parse_u32(input).map(|v| v as u64),
        2 => parse_u16(input).map(|v| v as u64),
        1 => parse_u8(input).map(|v| v as u64),
        otherwise => return Err(Error::UnsupportedAddressSize(otherwise)),
    }
}

/// Parse an address-sized integer, and return it as a `usize`.
#[doc(hidden)]
#[inline]
pub fn parse_address_as_offset<Endian>(input: &mut EndianBuf<Endian>,
                                       address_size: u8)
                                       -> Result<usize>
    where Endian: Endianity
{
    parse_address(input, address_size).and_then(u64_to_offset)
}

/// Parse a null-terminated slice from the input.
#[doc(hidden)]
#[inline]
pub fn parse_null_terminated_string<'input, Endian>(input: &mut EndianBuf<'input, Endian>)
                                                    -> Result<&'input ffi::CStr>
    where Endian: Endianity
{
    if let Some(idx) = input.find(0) {
        let slice = &take(idx + 1, input)?;
        let cstr = unsafe {
            // It is safe to use the unchecked variant here because we know we
            // grabbed the index of the first null byte in the input and
            // therefore there can't be any interior null bytes in this slice.
            ffi::CStr::from_bytes_with_nul_unchecked(slice.buf())
        };
        Ok(cstr)
    } else {
        Err(Error::UnexpectedEof)
    }
}

/// Parse a `DW_EH_PE_*` pointer encoding.
#[doc(hidden)]
#[inline]
pub fn parse_pointer_encoding<Endian>(input: &mut EndianBuf<Endian>) -> Result<constants::DwEhPe>
    where Endian: Endianity
{
    let eh_pe = parse_u8(input)?;
    let eh_pe = constants::DwEhPe(eh_pe);

    if eh_pe.is_valid_encoding() {
        Ok(eh_pe)
    } else {
        Err(Error::UnknownPointerEncoding)
    }
}

/// A decoded pointer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Pointer {
    /// This value is the decoded pointer value.
    Direct(u64),

    /// This value is *not* the pointer value, but points to the address of
    /// where the real pointer value lives. In other words, deref this pointer
    /// to get the real pointer value.
    ///
    /// Chase this pointer at your own risk: do you trust the DWARF data it came
    /// from?
    Indirect(u64),
}

impl Default for Pointer {
    fn default() -> Self {
        Pointer::Direct(0)
    }
}

impl Into<u64> for Pointer {
    fn into(self) -> u64 {
        match self {
            Pointer::Direct(p) |
            Pointer::Indirect(p) => p,
        }
    }
}

impl Pointer {
    fn new(encoding: constants::DwEhPe, address: u64) -> Pointer {
        if encoding.is_indirect() {
            Pointer::Indirect(address)
        } else {
            Pointer::Direct(address)
        }
    }
}

pub fn parse_encoded_pointer<'bases, 'input, Endian>(encoding: constants::DwEhPe,
                                                     bases: &'bases BaseAddresses,
                                                     address_size: u8,
                                                     section: EndianBuf<'input, Endian>,
                                                     input: &mut EndianBuf<'input, Endian>)
                                                     -> Result<Pointer>
    where Endian: Endianity
{
    fn parse_data<Endian>(encoding: constants::DwEhPe,
                          address_size: u8,
                          input: &mut EndianBuf<Endian>)
                          -> Result<u64>
        where Endian: Endianity
    {
        // We should never be called with an invalid encoding: parse_encoded_pointer
        // checks validity for us.
        debug_assert!(encoding.is_valid_encoding());

        match encoding.format() {
            // Unsigned variants.
            constants::DW_EH_PE_absptr => parse_address(input, address_size),
            constants::DW_EH_PE_uleb128 => parse_unsigned_leb(input),
            constants::DW_EH_PE_udata2 => parse_u16(input).map(|a| a as u64),
            constants::DW_EH_PE_udata4 => parse_u32(input).map(|a| a as u64),
            constants::DW_EH_PE_udata8 => parse_u64(input),

            // Signed variants. Here we sign extend the values (happens by
            // default when casting a signed integer to a larger range integer
            // in Rust), return them as u64, and rely on wrapping addition to do
            // the right thing when adding these offsets to their bases.
            constants::DW_EH_PE_sleb128 => parse_signed_leb(input).map(|a| a as u64),
            constants::DW_EH_PE_sdata2 => parse_i16(input).map(|a| a as u64),
            constants::DW_EH_PE_sdata4 => parse_i32(input).map(|a| a as u64),
            constants::DW_EH_PE_sdata8 => parse_i64(input).map(|a| a as u64),

            // That was all of the valid encoding formats.
            _ => unreachable!(),
        }
    }

    if !encoding.is_valid_encoding() {
        return Err(Error::UnknownPointerEncoding);
    }

    if encoding == constants::DW_EH_PE_omit {
        return Ok(Pointer::Direct(0));
    }

    match encoding.application() {
        constants::DW_EH_PE_absptr => {
            let addr = parse_data(encoding, address_size, input)?;
            Ok(Pointer::new(encoding, addr.into()))
        }
        constants::DW_EH_PE_pcrel => {
            if let Some(cfi) = bases.cfi {
                let offset_from_section = input.offset_from(section);
                let offset = parse_data(encoding, address_size, input)?;
                let p = cfi.wrapping_add(offset_from_section as u64)
                    .wrapping_add(offset);
                Ok(Pointer::new(encoding, p))
            } else {
                Err(Error::CfiRelativePointerButCfiBaseIsUndefined)
            }
        }
        constants::DW_EH_PE_textrel => {
            if let Some(text) = bases.text {
                let offset = parse_data(encoding, address_size, input)?;
                Ok(Pointer::new(encoding, text.wrapping_add(offset)))
            } else {
                Err(Error::TextRelativePointerButTextBaseIsUndefined)
            }
        }
        constants::DW_EH_PE_datarel => {
            if let Some(data) = bases.data {
                let offset = parse_data(encoding, address_size, input)?;
                Ok(Pointer::new(encoding, data.wrapping_add(offset)))
            } else {
                Err(Error::DataRelativePointerButDataBaseIsUndefined)
            }
        }
        constants::DW_EH_PE_funcrel => {
            let func = bases.func.borrow();
            if let Some(func) = *func {
                let offset = parse_data(encoding, address_size, input)?;
                Ok(Pointer::new(encoding, func.wrapping_add(offset)))
            } else {
                Err(Error::FuncRelativePointerInBadContext)
            }
        }
        constants::DW_EH_PE_aligned => Err(Error::UnsupportedPointerEncoding),
        _ => unreachable!(),
    }
}

/// An offset into the `.debug_macinfo` section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DebugMacinfoOffset(pub usize);

/// Whether the format of a compilation unit is 32- or 64-bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// 64-bit DWARF
    Dwarf64,
    /// 32-bit DWARF
    Dwarf32,
}

const MAX_DWARF_32_UNIT_LENGTH: u64 = 0xfffffff0;

const DWARF_64_INITIAL_UNIT_LENGTH: u64 = 0xffffffff;

/// Parse the compilation unit header's length.
#[doc(hidden)]
pub fn parse_initial_length<Endian>(input: &mut EndianBuf<Endian>) -> Result<(u64, Format)>
    where Endian: Endianity
{
    let val = parse_u32_as_u64(input)?;
    if val < MAX_DWARF_32_UNIT_LENGTH {
        Ok((val, Format::Dwarf32))
    } else if val == DWARF_64_INITIAL_UNIT_LENGTH {
        let val = parse_u64(input)?;
        Ok((val, Format::Dwarf64))
    } else {
        Err(Error::UnknownReservedLength)
    }
}

/// Parse the size of addresses (in bytes) on the target architecture.
pub fn parse_address_size<Endian>(input: &mut EndianBuf<Endian>) -> Result<u8>
    where Endian: Endianity
{
    parse_u8(input)
}

/// Take a slice of size `bytes` from the input.
#[inline]
pub fn take<'input, Endian>(bytes: usize,
                            input: &mut EndianBuf<'input, Endian>)
                            -> Result<EndianBuf<'input, Endian>>
    where Endian: Endianity
{
    if input.len() < bytes {
        Err(Error::UnexpectedEof)
    } else {
        let val = input.range_to(..bytes);
        *input = input.range_from(bytes..);
        Ok(val)
    }
}

/// Parse a length as an unsigned LEB128 from the input, then take
/// that many bytes from the input.  These bytes are returned as the
/// second element of the result tuple.
#[doc(hidden)]
pub fn parse_length_uleb_value<'input, Endian>(input: &mut EndianBuf<'input, Endian>)
                                               -> Result<EndianBuf<'input, Endian>>
    where Endian: Endianity
{
    let len = parse_unsigned_leb(input)?;
    take(len as usize, input)
}

#[cfg(test)]
mod tests {
    extern crate test_assembler;

    use super::*;
    use cfi::BaseAddresses;
    use constants;
    use endianity::{EndianBuf, LittleEndian};
    use self::test_assembler::{Endian, Section};
    use std::cell::RefCell;
    use test_util::GimliSectionMethods;

    #[test]
    fn test_parse_initial_length_32_ok() {
        let section = Section::with_endian(Endian::Little).L32(0x78563412);
        let buf = section.get_contents().unwrap();

        let input = &mut EndianBuf::<LittleEndian>::new(&buf);
        match parse_initial_length(input) {
            Ok((length, format)) => {
                assert_eq!(input.len(), 0);
                assert_eq!(format, Format::Dwarf32);
                assert_eq!(0x78563412, length);
            }
            otherwise => panic!("Unexpected result: {:?}", otherwise),
        }
    }

    #[test]
    fn test_parse_initial_length_64_ok() {
        let section = Section::with_endian(Endian::Little)
            // Dwarf_64_INITIAL_UNIT_LENGTH
            .L32(0xffffffff)
            // Actual length
            .L64(0xffdebc9a78563412);
        let buf = section.get_contents().unwrap();

        let input = &mut EndianBuf::<LittleEndian>::new(&buf);
        match parse_initial_length(input) {
            Ok((length, format)) => {
                assert_eq!(input.len(), 0);
                assert_eq!(format, Format::Dwarf64);
                assert_eq!(0xffdebc9a78563412, length);
            }
            otherwise => panic!("Unexpected result: {:?}", otherwise),
        }
    }

    #[test]
    fn test_parse_initial_length_unknown_reserved_value() {
        let section = Section::with_endian(Endian::Little).L32(0xfffffffe);
        let buf = section.get_contents().unwrap();

        match parse_initial_length(&mut EndianBuf::<LittleEndian>::new(&buf)) {
            Err(Error::UnknownReservedLength) => assert!(true),
            otherwise => panic!("Unexpected result: {:?}", otherwise),
        };
    }

    #[test]
    fn test_parse_initial_length_incomplete() {
        let buf = [0xff, 0xff, 0xff]; // Need at least 4 bytes.

        match parse_initial_length(&mut EndianBuf::<LittleEndian>::new(&buf)) {
            Err(Error::UnexpectedEof) => assert!(true),
            otherwise => panic!("Unexpected result: {:?}", otherwise),
        };
    }

    #[test]
    fn test_parse_initial_length_64_incomplete() {
        let section = Section::with_endian(Endian::Little)
            // Dwarf_64_INITIAL_UNIT_LENGTH
            .L32(0xffffffff)
            // Actual length is not long enough.
            .L32(0x78563412);
        let buf = section.get_contents().unwrap();

        match parse_initial_length(&mut EndianBuf::<LittleEndian>::new(&buf)) {
            Err(Error::UnexpectedEof) => assert!(true),
            otherwise => panic!("Unexpected result: {:?}", otherwise),
        };
    }

    #[test]
    fn test_parse_offset_32() {
        let section = Section::with_endian(Endian::Little).L32(0x01234567);
        let buf = section.get_contents().unwrap();

        let input = &mut EndianBuf::<LittleEndian>::new(&buf);
        match parse_offset(input, Format::Dwarf32) {
            Ok(val) => {
                assert_eq!(input.len(), 0);
                assert_eq!(val, 0x01234567);
            }
            otherwise => panic!("Unexpected result: {:?}", otherwise),
        };
    }

    #[test]
    fn test_parse_offset_64_small() {
        let section = Section::with_endian(Endian::Little).L64(0x01234567);
        let buf = section.get_contents().unwrap();

        let input = &mut EndianBuf::<LittleEndian>::new(&buf);
        match parse_offset(input, Format::Dwarf64) {
            Ok(val) => {
                assert_eq!(input.len(), 0);
                assert_eq!(val, 0x01234567);
            }
            otherwise => panic!("Unexpected result: {:?}", otherwise),
        };
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn test_parse_offset_64_large() {
        let section = Section::with_endian(Endian::Little).L64(0x0123456789abcdef);
        let buf = section.get_contents().unwrap();

        let input = &mut EndianBuf::<LittleEndian>::new(&buf);
        match parse_offset(input, Format::Dwarf64) {
            Ok(val) => {
                assert_eq!(input.len(), 0);
                assert_eq!(val, 0x0123456789abcdef);
            }
            otherwise => panic!("Unexpected result: {:?}", otherwise),
        };
    }

    #[test]
    #[cfg(target_pointer_width = "32")]
    fn test_parse_offset_64_large() {
        let section = Section::with_endian(Endian::Little).L64(0x0123456789abcdef);
        let buf = section.get_contents().unwrap();

        match parse_offset(EndianBuf::<LittleEndian>::new(&buf), Format::Dwarf64) {
            Err(Error::UnsupportedOffset) => assert!(true),
            otherwise => panic!("Unexpected result: {:?}", otherwise),
        };
    }

    #[test]
    fn test_parse_address_size_ok() {
        let buf = [0x04];

        match parse_address_size(&mut EndianBuf::<LittleEndian>::new(&buf)) {
            Ok(val) => assert_eq!(val, 4),
            otherwise => panic!("Unexpected result: {:?}", otherwise),
        };
    }

    #[test]
    fn test_parse_pointer_encoding_ok() {
        use endianity::NativeEndian;
        let expected = constants::DwEhPe(constants::DW_EH_PE_uleb128.0 |
                                         constants::DW_EH_PE_pcrel.0);
        let input = [expected.0, 1, 2, 3, 4];
        let input = &mut EndianBuf::<NativeEndian>::new(&input);
        assert_eq!(parse_pointer_encoding(input), Ok(expected));
        assert_eq!(*input, EndianBuf::new(&[1, 2, 3, 4]));
    }

    #[test]
    fn test_parse_pointer_encoding_bad_encoding() {
        use endianity::NativeEndian;
        let expected = constants::DwEhPe((constants::DW_EH_PE_sdata8.0 + 1) |
                                         constants::DW_EH_PE_pcrel.0);
        let input = [expected.0, 1, 2, 3, 4];
        let input = &mut EndianBuf::<NativeEndian>::new(&input);
        assert_eq!(Err(Error::UnknownPointerEncoding),
                   parse_pointer_encoding(input));
    }

    #[test]
    fn test_parse_encoded_pointer_absptr() {
        let encoding = constants::DW_EH_PE_absptr;
        let bases = Default::default();
        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];

        let input = Section::with_endian(Endian::Little)
            .L32(0xf00df00d)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(0xf00df00d)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_pcrel() {
        let encoding = constants::DW_EH_PE_pcrel;

        let bases = BaseAddresses::default().set_cfi(0x100);

        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];

        let input = Section::with_endian(Endian::Little)
            .append_repeated(0, 0x10)
            .L32(0x1)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input.range_from(0x10..);

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(0x111)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_pcrel_undefined() {
        let encoding = constants::DW_EH_PE_pcrel;
        let bases = BaseAddresses::default();
        let address_size = 4;

        let input = Section::with_endian(Endian::Little).L32(0x1);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Err(Error::CfiRelativePointerButCfiBaseIsUndefined));
    }

    #[test]
    fn test_parse_encoded_pointer_textrel() {
        let encoding = constants::DW_EH_PE_textrel;

        let bases = BaseAddresses::default().set_text(0x10);

        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];

        let input = Section::with_endian(Endian::Little)
            .L32(0x1)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(0x11)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_textrel_undefined() {
        let encoding = constants::DW_EH_PE_textrel;
        let bases = BaseAddresses::default();
        let address_size = 4;

        let input = Section::with_endian(Endian::Little).L32(0x1);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Err(Error::TextRelativePointerButTextBaseIsUndefined));
    }

    #[test]
    fn test_parse_encoded_pointer_datarel() {
        let encoding = constants::DW_EH_PE_datarel;

        let bases = BaseAddresses::default().set_data(0x10);

        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];

        let input = Section::with_endian(Endian::Little)
            .L32(0x1)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(0x11)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_datarel_undefined() {
        let encoding = constants::DW_EH_PE_datarel;
        let bases = BaseAddresses::default();
        let address_size = 4;

        let input = Section::with_endian(Endian::Little).L32(0x1);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Err(Error::DataRelativePointerButDataBaseIsUndefined));
    }

    #[test]
    fn test_parse_encoded_pointer_funcrel() {
        let encoding = constants::DW_EH_PE_funcrel;

        let mut bases = BaseAddresses::default();
        bases.func = RefCell::new(Some(0x10));

        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];

        let input = Section::with_endian(Endian::Little)
            .L32(0x1)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(0x11)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_funcrel_undefined() {
        let encoding = constants::DW_EH_PE_funcrel;
        let bases = BaseAddresses::default();
        let address_size = 4;

        let input = Section::with_endian(Endian::Little).L32(0x1);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Err(Error::FuncRelativePointerInBadContext));
    }

    #[test]
    fn test_parse_encoded_pointer_uleb128() {
        let encoding = constants::DwEhPe(constants::DW_EH_PE_absptr.0 |
                                         constants::DW_EH_PE_uleb128.0);
        let bases = BaseAddresses::default();
        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];

        let input = Section::with_endian(Endian::Little)
            .uleb(0x123456)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(0x123456)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_udata2() {
        let encoding = constants::DwEhPe(constants::DW_EH_PE_absptr.0 |
                                         constants::DW_EH_PE_udata2.0);
        let bases = BaseAddresses::default();
        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];

        let input = Section::with_endian(Endian::Little)
            .L16(0x1234)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(0x1234)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_udata4() {
        let encoding = constants::DwEhPe(constants::DW_EH_PE_absptr.0 |
                                         constants::DW_EH_PE_udata4.0);
        let bases = BaseAddresses::default();
        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];

        let input = Section::with_endian(Endian::Little)
            .L32(0x12345678)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(0x12345678)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_udata8() {
        let encoding = constants::DwEhPe(constants::DW_EH_PE_absptr.0 |
                                         constants::DW_EH_PE_udata8.0);
        let bases = BaseAddresses::default();
        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];

        let input = Section::with_endian(Endian::Little)
            .L64(0x1234567812345678)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(0x1234567812345678)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_sleb128() {
        let encoding = constants::DwEhPe(constants::DW_EH_PE_textrel.0 |
                                         constants::DW_EH_PE_sleb128.0);
        let bases = BaseAddresses::default().set_text(0x11111111);
        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];

        let input = Section::with_endian(Endian::Little)
            .sleb(-0x1111)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(0x11110000)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_sdata2() {
        let encoding = constants::DwEhPe(constants::DW_EH_PE_absptr.0 |
                                         constants::DW_EH_PE_sdata2.0);
        let bases = BaseAddresses::default();
        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];
        let expected = 0x111 as i16;

        let input = Section::with_endian(Endian::Little)
            .L16(expected as u16)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(expected as u64)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_sdata4() {
        let encoding = constants::DwEhPe(constants::DW_EH_PE_absptr.0 |
                                         constants::DW_EH_PE_sdata4.0);
        let bases = BaseAddresses::default();
        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];
        let expected = 0x1111111 as i32;

        let input = Section::with_endian(Endian::Little)
            .L32(expected as u32)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(expected as u64)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_sdata8() {
        let encoding = constants::DwEhPe(constants::DW_EH_PE_absptr.0 |
                                         constants::DW_EH_PE_sdata8.0);
        let bases = BaseAddresses::default();
        let address_size = 4;
        let expected_rest = [1, 2, 3, 4];
        let expected = -0x11111112222222 as i64;

        let input = Section::with_endian(Endian::Little)
            .L64(expected as u64)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Direct(expected as u64)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }

    #[test]
    fn test_parse_encoded_pointer_omit() {
        let encoding = constants::DW_EH_PE_omit;
        let bases = BaseAddresses::default();
        let address_size = 4;

        let input = Section::with_endian(Endian::Little).L32(0x1);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::default()));
        assert_eq!(rest, input);
    }

    #[test]
    fn test_parse_encoded_pointer_bad_encoding() {
        let encoding = constants::DwEhPe(constants::DW_EH_PE_sdata8.0 + 1);
        let bases = BaseAddresses::default();
        let address_size = 4;

        let input = Section::with_endian(Endian::Little).L32(0x1);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Err(Error::UnknownPointerEncoding));
    }

    #[test]
    fn test_parse_encoded_pointer_aligned() {
        // FIXME: support this encoding!

        let encoding = constants::DW_EH_PE_aligned;
        let bases = BaseAddresses::default();
        let address_size = 4;

        let input = Section::with_endian(Endian::Little).L32(0x1);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Err(Error::UnsupportedPointerEncoding));
    }

    #[test]
    fn test_parse_encoded_pointer_indirect() {
        let expected_rest = [1, 2, 3, 4];

        let encoding = constants::DW_EH_PE_indirect;
        let bases = BaseAddresses::default();
        let address_size = 4;

        let input = Section::with_endian(Endian::Little)
            .L32(0x12345678)
            .append_bytes(&expected_rest);
        let input = input.get_contents().unwrap();
        let input = EndianBuf::<LittleEndian>::new(&input);
        let mut rest = input;

        assert_eq!(parse_encoded_pointer(encoding, &bases, address_size, input, &mut rest),
                   Ok(Pointer::Indirect(0x12345678)));
        assert_eq!(rest, EndianBuf::new(&expected_rest));
    }
}
