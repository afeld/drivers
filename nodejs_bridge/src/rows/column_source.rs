use arrow::array::Array;
use arrow::datatypes::{DataType, Field};
use std::str::FromStr;

use super::numeric::{FloatColumn, IntColumn};
use super::temporal::{TimeColumn, TimestampColumn};

pub(super) struct ColumnSource<'a> {
    pub(super) field: &'a Field,
    pub(super) column: &'a dyn Array,
    pub(super) label: String,
}

impl ColumnSource<'_> {
    pub(super) fn array<T: Array + Clone + 'static>(&self) -> Result<T, String> {
        self.column
            .as_any()
            .downcast_ref::<T>()
            .cloned()
            .ok_or_else(|| self.unsupported())
    }

    pub(super) fn metadata<T: FromStr>(&self, key: &str) -> Result<T, String> {
        let raw = self
            .field
            .metadata()
            .get(key)
            .ok_or_else(|| format!("{} is missing {key} metadata", self.label))?;
        raw.parse()
            .map_err(|_| format!("{} has invalid {key} metadata {raw:?}", self.label))
    }

    pub(super) fn int_column(&self) -> Result<IntColumn, String> {
        Ok(match self.column.data_type() {
            DataType::Int8 => IntColumn::I8(self.array()?),
            DataType::Int16 => IntColumn::I16(self.array()?),
            DataType::Int32 => IntColumn::I32(self.array()?),
            DataType::Int64 => IntColumn::I64(self.array()?),
            DataType::Decimal128(_, _) => IntColumn::Decimal(self.array()?),
            _ => return Err(self.unsupported()),
        })
    }

    pub(super) fn float_column(&self) -> Result<FloatColumn, String> {
        Ok(match self.column.data_type() {
            DataType::Float32 => FloatColumn::F32(self.array()?),
            DataType::Float64 => FloatColumn::F64(self.array()?),
            _ => return Err(self.unsupported()),
        })
    }

    pub(super) fn time_column(&self) -> Result<TimeColumn, String> {
        Ok(match self.column.data_type() {
            DataType::Int32 => TimeColumn::I32(self.array()?),
            DataType::Int64 => TimeColumn::I64(self.array()?),
            _ => return Err(self.unsupported()),
        })
    }

    pub(super) fn timestamp_column(&self) -> Result<TimestampColumn, String> {
        Ok(match self.column.data_type() {
            DataType::Struct(_) => TimestampColumn::Struct(self.array()?),
            DataType::Int64 => TimestampColumn::I64(self.array()?),
            _ => return Err(self.unsupported()),
        })
    }

    fn unsupported(&self) -> String {
        format!(
            "{} has unsupported Arrow type {}",
            self.label,
            self.column.data_type()
        )
    }
}
