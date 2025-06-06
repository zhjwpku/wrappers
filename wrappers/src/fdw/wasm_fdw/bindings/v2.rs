use pgrx::{
    datum::datetime_support::to_timestamp,
    prelude::{Date, Timestamp, TimestampWithTimeZone},
    AnyNumeric, JsonB,
};
use uuid::Uuid;
use wasmtime::component::bindgen;
use wasmtime::Error as WasmError;

use super::{PG_EPOCH_MS, PG_EPOCH_SEC};
use crate::stats::Metric as HostMetric;
use supabase_wrappers::prelude::{
    Cell as HostCell, ImportForeignSchemaStmt as HostImportForeignSchemaStmt,
    ImportSchemaType as HostImportSchemaType, Param as HostParam, Value as HostValue,
};

bindgen!("wrappers" in "../wasm-wrappers/wit/v2");

use self::supabase::wrappers::{
    stats::Metric as GuestMetric,
    types::{
        Cell as GuestCell, ImportForeignSchemaStmt as GuestImportForeignSchemaStmt,
        ImportSchemaType as GuestImportSchemaType, Param as GuestParam, Value as GuestValue,
    },
};

impl TryFrom<GuestCell> for HostCell {
    type Error = WasmError;

    fn try_from(value: GuestCell) -> Result<Self, Self::Error> {
        match value {
            GuestCell::Bool(v) => Ok(Self::Bool(v)),
            GuestCell::I8(v) => Ok(Self::I8(v)),
            GuestCell::I16(v) => Ok(Self::I16(v)),
            GuestCell::F32(v) => Ok(Self::F32(v)),
            GuestCell::I32(v) => Ok(Self::I32(v)),
            GuestCell::F64(v) => Ok(Self::F64(v)),
            GuestCell::I64(v) => Ok(Self::I64(v)),
            GuestCell::Numeric(v) => {
                let ret = AnyNumeric::try_from(v).map(Self::Numeric)?;
                Ok(ret)
            }
            GuestCell::String(v) => Ok(Self::String(v.clone())),
            GuestCell::Date(v) => {
                let ts = to_timestamp(v as f64);
                Ok(Self::Date(Date::from(ts)))
            }
            // convert 'pg epoch' (2000-01-01 00:00:00) to unix epoch
            GuestCell::Timestamp(v) => Timestamp::try_from(v - PG_EPOCH_MS)
                .map(Self::Timestamp)
                .map_err(Self::Error::msg),
            GuestCell::Timestamptz(v) => TimestampWithTimeZone::try_from(v - PG_EPOCH_MS)
                .map(Self::Timestamptz)
                .map_err(Self::Error::msg),
            GuestCell::Json(v) => {
                let ret = serde_json::from_str(&v).map(|j| Self::Json(JsonB(j)))?;
                Ok(ret)
            }
            GuestCell::Uuid(v) => Uuid::try_parse(&v)
                .map(|u| Self::Uuid(pgrx::Uuid::from_bytes(*u.as_bytes())))
                .map_err(Self::Error::msg),
            _ => todo!("Add more type support from guest cell to host cell"),
        }
    }
}

impl From<&HostCell> for GuestCell {
    fn from(value: &HostCell) -> Self {
        match value {
            HostCell::Bool(v) => Self::Bool(*v),
            HostCell::I8(v) => Self::I8(*v),
            HostCell::I16(v) => Self::I16(*v),
            HostCell::F32(v) => Self::F32(*v),
            HostCell::I32(v) => Self::I32(*v),
            HostCell::F64(v) => Self::F64(*v),
            HostCell::I64(v) => Self::I64(*v),
            HostCell::Numeric(v) => Self::Numeric(v.clone().try_into().unwrap()),
            HostCell::String(v) => Self::String(v.clone()),
            HostCell::Date(v) => {
                // convert 'pg epoch' (2000-01-01 00:00:00) to unix epoch
                let ts = Timestamp::from(*v);
                Self::Date(ts.into_inner() / 1_000_000 + PG_EPOCH_SEC)
            }
            HostCell::Timestamp(v) => {
                // convert 'pg epoch' (2000-01-01 00:00:00) in macroseconds to unix epoch
                Self::Timestamp(v.into_inner() + PG_EPOCH_MS)
            }
            HostCell::Timestamptz(v) => {
                // convert 'pg epoch' (2000-01-01 00:00:00) in macroseconds to unix epoch
                Self::Timestamptz(v.into_inner() + PG_EPOCH_MS)
            }
            HostCell::Json(v) => Self::Json(v.0.to_string()),
            HostCell::Uuid(v) => Self::Uuid(v.to_string()),
            _ => todo!("Add more type support from host cell to guest cell"),
        }
    }
}

impl From<HostValue> for GuestValue {
    fn from(value: HostValue) -> Self {
        match value {
            HostValue::Cell(c) => Self::Cell(GuestCell::from(&c)),
            HostValue::Array(a) => {
                let a: Vec<GuestCell> = a.iter().map(GuestCell::from).collect();
                Self::Array(a)
            }
        }
    }
}

impl From<HostParam> for GuestParam {
    fn from(value: HostParam) -> Self {
        Self {
            id: value.id as u32,
            type_oid: value.type_oid.to_u32(),
        }
    }
}

impl From<GuestMetric> for HostMetric {
    fn from(value: GuestMetric) -> Self {
        match value {
            GuestMetric::CreateTimes => HostMetric::CreateTimes,
            GuestMetric::RowsIn => HostMetric::RowsIn,
            GuestMetric::RowsOut => HostMetric::RowsOut,
            GuestMetric::BytesIn => HostMetric::BytesIn,
            GuestMetric::BytesOut => HostMetric::BytesOut,
        }
    }
}

impl From<HostImportSchemaType> for GuestImportSchemaType {
    fn from(value: HostImportSchemaType) -> Self {
        match value {
            HostImportSchemaType::FdwImportSchemaAll => GuestImportSchemaType::All,
            HostImportSchemaType::FdwImportSchemaLimitTo => GuestImportSchemaType::LimitTo,
            HostImportSchemaType::FdwImportSchemaExcept => GuestImportSchemaType::Except,
        }
    }
}

impl From<HostImportForeignSchemaStmt> for GuestImportForeignSchemaStmt {
    fn from(value: HostImportForeignSchemaStmt) -> Self {
        Self {
            server_name: value.server_name.clone(),
            remote_schema: value.remote_schema.clone(),
            local_schema: value.local_schema.clone(),
            list_type: GuestImportSchemaType::from(value.list_type),
            table_list: value.table_list.clone(),
        }
    }
}
