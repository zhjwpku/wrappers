use crate::stats;
#[allow(deprecated)]
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Utc};
use clickhouse_rs::{
    types,
    types::Block,
    types::SqlType,
    types::Value as ChValue,
    types::{i256, u256},
    ClientHandle, Pool,
};
use pgrx::datum::numeric::AnyNumeric;
use pgrx::prelude::to_timestamp;
use regex::{Captures, Regex};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use supabase_wrappers::prelude::*;

use super::{ClickHouseFdwError, ClickHouseFdwResult};

fn field_to_cell(row: &types::Row<types::Complex>, i: usize) -> ClickHouseFdwResult<Option<Cell>> {
    let sql_type = row.sql_type(i)?;
    match sql_type {
        SqlType::Bool => {
            let value = row.get::<bool, usize>(i)?;
            Ok(Some(Cell::Bool(value)))
        }
        SqlType::Int8 => {
            let value = row.get::<i8, usize>(i)?;
            Ok(Some(Cell::I8(value)))
        }
        SqlType::UInt8 => {
            // up-cast UInt8 to i16
            let value = row.get::<u8, usize>(i)?;
            Ok(Some(Cell::I16(value as i16)))
        }
        SqlType::Int16 => {
            let value = row.get::<i16, usize>(i)?;
            Ok(Some(Cell::I16(value)))
        }
        SqlType::UInt16 => {
            // up-cast UInt16 to i32
            let value = row.get::<u16, usize>(i)?;
            Ok(Some(Cell::I32(value as i32)))
        }
        SqlType::Int32 => {
            let value = row.get::<i32, usize>(i)?;
            Ok(Some(Cell::I32(value)))
        }
        SqlType::UInt32 => {
            // up-cast UInt32 to i64
            let value = row.get::<u32, usize>(i)?;
            Ok(Some(Cell::I64(value as i64)))
        }
        SqlType::Float32 => {
            let value = row.get::<f32, usize>(i)?;
            Ok(Some(Cell::F32(value)))
        }
        SqlType::Float64 => {
            let value = row.get::<f64, usize>(i)?;
            Ok(Some(Cell::F64(value)))
        }
        SqlType::Int64 => {
            let value = row.get::<i64, usize>(i)?;
            Ok(Some(Cell::I64(value)))
        }
        SqlType::UInt64 => {
            let value = row.get::<u64, usize>(i)?;
            Ok(Some(Cell::I64(value as i64)))
        }
        SqlType::Int128 => {
            let value = row.get::<i128, usize>(i)?;
            Ok(Some(Cell::Numeric(AnyNumeric::from(value))))
        }
        SqlType::UInt128 => {
            let value = row.get::<u128, usize>(i)?;
            Ok(Some(Cell::Numeric(AnyNumeric::from(value))))
        }
        SqlType::Int256 => {
            let value = row.get::<i256, usize>(i)?;
            Ok(Some(Cell::String(value.to_string())))
        }
        SqlType::UInt256 => {
            let value = row.get::<u256, usize>(i)?;
            Ok(Some(Cell::String(value.to_string())))
        }
        SqlType::Decimal(_u, _s) => {
            let value = row.get::<types::Decimal, usize>(i)?;
            let value_str = value.to_string();
            Ok(Some(Cell::Numeric(AnyNumeric::try_from(
                value_str.as_str(),
            )?)))
        }
        SqlType::String | SqlType::FixedString(_) => {
            let value = row.get::<String, usize>(i)?;
            Ok(Some(Cell::String(value)))
        }
        SqlType::Date => {
            let value = row.get::<NaiveDate, usize>(i)?;
            let dt =
                pgrx::prelude::Date::new(value.year(), value.month() as u8, value.day() as u8)?;
            Ok(Some(Cell::Date(dt)))
        }
        SqlType::DateTime(_) => {
            let value = row.get::<DateTime<_>, usize>(i)?;
            let ts = to_timestamp(value.timestamp() as f64);
            Ok(Some(Cell::Timestamp(ts.to_utc())))
        }
        SqlType::Uuid => {
            let value = row.get::<Uuid, usize>(i)?;
            Ok(Some(Cell::Uuid(pgrx::Uuid::from_bytes(*value.as_bytes()))))
        }
        SqlType::Array(SqlType::Bool) => {
            let value = row
                .get::<Vec<bool>, usize>(i)?
                .into_iter()
                .map(Some)
                .collect();
            Ok(Some(Cell::BoolArray(value)))
        }
        SqlType::Array(SqlType::Int16) => {
            let value = row
                .get::<Vec<i16>, usize>(i)?
                .into_iter()
                .map(Some)
                .collect();
            Ok(Some(Cell::I16Array(value)))
        }
        SqlType::Array(SqlType::Int32) => {
            let value = row
                .get::<Vec<i32>, usize>(i)?
                .into_iter()
                .map(Some)
                .collect();
            Ok(Some(Cell::I32Array(value)))
        }
        SqlType::Array(SqlType::Int64) => {
            let value = row
                .get::<Vec<i64>, usize>(i)?
                .into_iter()
                .map(Some)
                .collect();
            Ok(Some(Cell::I64Array(value)))
        }
        SqlType::Array(SqlType::Float32) => {
            let value = row
                .get::<Vec<f32>, usize>(i)?
                .into_iter()
                .map(Some)
                .collect();
            Ok(Some(Cell::F32Array(value)))
        }
        SqlType::Array(SqlType::Float64) => {
            let value = row
                .get::<Vec<f64>, usize>(i)?
                .into_iter()
                .map(Some)
                .collect();
            Ok(Some(Cell::F64Array(value)))
        }
        SqlType::Array(SqlType::String) => {
            let value = row
                .get::<Vec<String>, usize>(i)?
                .into_iter()
                .map(Some)
                .collect();
            Ok(Some(Cell::StringArray(value)))
        }
        SqlType::Nullable(v) => match v {
            SqlType::Bool => {
                let value = row.get::<Option<bool>, usize>(i)?;
                Ok(value.map(Cell::Bool))
            }
            SqlType::Int8 => {
                let value = row.get::<Option<i8>, usize>(i)?;
                Ok(value.map(Cell::I8))
            }
            SqlType::UInt8 => {
                let value = row.get::<Option<u8>, usize>(i)?;
                Ok(value.map(|t| Cell::I16(t as _)))
            }
            SqlType::Int16 => {
                let value = row.get::<Option<i16>, usize>(i)?;
                Ok(value.map(Cell::I16))
            }
            SqlType::UInt16 => {
                let value = row.get::<Option<u16>, usize>(i)?;
                Ok(value.map(|t| Cell::I32(t as _)))
            }
            SqlType::Int32 => {
                let value = row.get::<Option<i32>, usize>(i)?;
                Ok(value.map(Cell::I32))
            }
            SqlType::UInt32 => {
                let value = row.get::<Option<u32>, usize>(i)?;
                Ok(value.map(|t| Cell::I64(t as _)))
            }
            SqlType::Float32 => {
                let value = row.get::<Option<f32>, usize>(i)?;
                Ok(value.map(Cell::F32))
            }
            SqlType::Float64 => {
                let value = row.get::<Option<f64>, usize>(i)?;
                Ok(value.map(Cell::F64))
            }
            SqlType::Int64 => {
                let value = row.get::<Option<i64>, usize>(i)?;
                Ok(value.map(Cell::I64))
            }
            SqlType::UInt64 => {
                let value = row.get::<Option<u64>, usize>(i)?;
                Ok(value.map(|t| Cell::I64(t as _)))
            }
            SqlType::Int128 => {
                let value = row.get::<Option<i128>, usize>(i)?;
                Ok(value.map(|t| Cell::Numeric(AnyNumeric::from(t))))
            }
            SqlType::UInt128 => {
                let value = row.get::<Option<u128>, usize>(i)?;
                Ok(value.map(|t| Cell::Numeric(AnyNumeric::from(t))))
            }
            SqlType::Int256 => {
                let value = row.get::<Option<i256>, usize>(i)?;
                Ok(value.map(|t| Cell::String(t.to_string())))
            }
            SqlType::UInt256 => {
                let value = row.get::<Option<u256>, usize>(i)?;
                Ok(value.map(|t| Cell::String(t.to_string())))
            }
            SqlType::Decimal(_u, _s) => {
                let value = row.get::<Option<types::Decimal>, usize>(i)?;
                if let Some(value) = value {
                    let value_str = value.to_string();
                    Ok(Some(Cell::Numeric(AnyNumeric::try_from(
                        value_str.as_str(),
                    )?)))
                } else {
                    Ok(None)
                }
            }
            SqlType::String | SqlType::FixedString(_) => {
                let value = row.get::<Option<String>, usize>(i)?;
                Ok(value.map(Cell::String))
            }
            SqlType::Date => {
                let value = row.get::<Option<NaiveDate>, usize>(i)?;
                Ok(value
                    .map(|t| pgrx::prelude::Date::new(t.year(), t.month() as u8, t.day() as u8))
                    .transpose()?
                    .map(Cell::Date))
            }
            SqlType::DateTime(_) => {
                let value = row.get::<Option<DateTime<_>>, usize>(i)?;
                Ok(value.map(|t| {
                    let ts = to_timestamp(t.timestamp() as f64);
                    Cell::Timestamp(ts.to_utc())
                }))
            }
            SqlType::Uuid => {
                let value = row.get::<Option<Uuid>, usize>(i)?;
                Ok(value
                    .map(|t| pgrx::Uuid::from_bytes(*t.as_bytes()))
                    .map(Cell::Uuid))
            }
            _ => Err(ClickHouseFdwError::UnsupportedColumnType(
                sql_type.to_string().into(),
            )),
        },
        _ => Err(ClickHouseFdwError::UnsupportedColumnType(
            sql_type.to_string().into(),
        )),
    }
}

fn array_cell_to_clickhouse_value<T: Clone>(
    v: impl AsRef<[Option<T>]>,
    array_type: &'static SqlType,
    is_nullable: bool,
) -> ClickHouseFdwResult<ChValue>
where
    ChValue: From<T>,
{
    let v: Vec<ChValue> = v
        .as_ref()
        .iter()
        .flatten()
        .cloned()
        .map(ChValue::from)
        .collect();
    let arr = ChValue::Array(array_type, Arc::new(v));
    let val = if is_nullable {
        ChValue::Nullable(either::Either::Right(Box::new(arr)))
    } else {
        arr
    };
    Ok(val)
}

#[wrappers_fdw(
    version = "0.1.7",
    author = "Supabase",
    website = "https://github.com/supabase/wrappers/tree/main/wrappers/src/fdw/clickhouse_fdw",
    error_type = "ClickHouseFdwError"
)]
pub(crate) struct ClickHouseFdw {
    rt: Runtime,
    conn_str: String,
    client: Option<ClientHandle>,
    table: String,
    rowid_col: String,
    tgt_cols: Vec<Column>,
    scan_blk: Option<Block<types::Complex>>,
    row_idx: usize,
    params: Vec<Qual>,
}

impl ClickHouseFdw {
    const FDW_NAME: &'static str = "ClickHouseFdw";

    fn create_client(&mut self) -> ClickHouseFdwResult<()> {
        let pool = Pool::new(self.conn_str.as_str());
        self.client = Some(self.rt.block_on(pool.get_handle())?);
        Ok(())
    }

    fn replace_all_params(
        &mut self,
        re: &Regex,
        mut replacement: impl FnMut(&Captures) -> ClickHouseFdwResult<String>,
    ) -> ClickHouseFdwResult<String> {
        let mut new = String::with_capacity(self.table.len());
        let mut last_match = 0;
        for caps in re.captures_iter(&self.table) {
            let m = caps.get(0).unwrap();
            new.push_str(&self.table[last_match..m.start()]);
            new.push_str(&replacement(&caps)?);
            last_match = m.end();
        }
        new.push_str(&self.table[last_match..]);
        Ok(new)
    }

    fn deparse(
        &mut self,
        quals: &[Qual],
        columns: &[Column],
        sorts: &[Sort],
        limit: &Option<Limit>,
    ) -> ClickHouseFdwResult<String> {
        let table = if self.table.starts_with('(') {
            let re = Regex::new(r"\$\{(\w+)\}").unwrap();
            let mut params = Vec::new();
            let mut replacement = |caps: &Captures| -> ClickHouseFdwResult<String> {
                let param = &caps[1];
                for qual in quals.iter() {
                    if qual.field == param {
                        params.push(qual.clone());
                        match &qual.value {
                            Value::Cell(cell) => return Ok(cell.to_string()),
                            Value::Array(arr) => {
                                return Err(ClickHouseFdwError::NoArrayParameter(format!(
                                    "{:?}",
                                    arr
                                )))
                            }
                        }
                    }
                }
                Err(ClickHouseFdwError::UnmatchedParameter(param.to_owned()))
            };
            let s = self.replace_all_params(&re, &mut replacement)?;
            self.params = params;
            s
        } else {
            self.table.clone()
        };

        let tgts = if columns.is_empty() {
            "*".to_string()
        } else {
            columns
                .iter()
                .filter(|c| !self.params.iter().any(|p| p.field == c.name))
                .map(|c| c.name.clone())
                .collect::<Vec<String>>()
                .join(", ")
        };

        let mut sql = format!("select {} from {}", tgts, &table);

        if !quals.is_empty() {
            let cond = quals
                .iter()
                .filter(|q| !self.params.iter().any(|p| p.field == q.field))
                .map(|q| q.deparse())
                .collect::<Vec<String>>()
                .join(" and ");

            if !cond.is_empty() {
                sql.push_str(&format!(" where {}", cond));
            }
        }

        // push down sorts
        if !sorts.is_empty() {
            let order_by = sorts
                .iter()
                .map(|sort| sort.deparse())
                .collect::<Vec<String>>()
                .join(", ");
            sql.push_str(&format!(" order by {}", order_by));
        }

        // push down limits
        // Note: Postgres will take limit and offset locally after reading rows
        // from remote, so we calculate the real limit and only use it without
        // pushing down offset.
        if let Some(limit) = limit {
            let real_limit = limit.offset + limit.count;
            sql.push_str(&format!(" limit {}", real_limit));
        }

        Ok(sql)
    }
}

impl ForeignDataWrapper<ClickHouseFdwError> for ClickHouseFdw {
    fn new(server: ForeignServer) -> ClickHouseFdwResult<Self> {
        let rt = create_async_runtime()?;
        let conn_str = match server.options.get("conn_string") {
            Some(conn_str) => conn_str.to_owned(),
            None => {
                let conn_str_id = require_option("conn_string_id", &server.options)?;
                get_vault_secret(conn_str_id).unwrap_or_default()
            }
        };

        stats::inc_stats(Self::FDW_NAME, stats::Metric::CreateTimes, 1);

        Ok(Self {
            rt,
            conn_str,
            client: None,
            table: String::default(),
            rowid_col: String::default(),
            tgt_cols: Vec::new(),
            scan_blk: None,
            row_idx: 0,
            params: Vec::new(),
        })
    }

    fn begin_scan(
        &mut self,
        quals: &[Qual],
        columns: &[Column],
        sorts: &[Sort],
        limit: &Option<Limit>,
        options: &HashMap<String, String>,
    ) -> ClickHouseFdwResult<()> {
        self.create_client()?;

        self.table = require_option("table", options)?.to_string();
        self.tgt_cols = columns.to_vec();
        self.row_idx = 0;

        let sql = self.deparse(quals, columns, sorts, limit)?;

        if let Some(ref mut client) = self.client {
            // for simplicity purpose, we fetch whole query result to local,
            // may need optimization in the future.
            let block = self.rt.block_on(client.query(&sql).fetch_all())?;
            stats::inc_stats(
                Self::FDW_NAME,
                stats::Metric::RowsIn,
                block.row_count() as i64,
            );
            stats::inc_stats(
                Self::FDW_NAME,
                stats::Metric::RowsOut,
                block.row_count() as i64,
            );
            self.scan_blk = Some(block);
        }

        Ok(())
    }

    fn iter_scan(&mut self, row: &mut Row) -> ClickHouseFdwResult<Option<()>> {
        if let Some(block) = &self.scan_blk {
            let mut rows = block.rows();

            if let Some(src_row) = rows.nth(self.row_idx) {
                for tgt_col in &self.tgt_cols {
                    if let Some(param) = self.params.iter().find(|&p| p.field == tgt_col.name) {
                        if let Value::Cell(cell) = &param.value {
                            row.push(&tgt_col.name, Some(cell.clone()));
                        }
                        continue;
                    }

                    let cell = if let Some((i, _)) = block
                        .columns()
                        .iter()
                        .enumerate()
                        .find(|(_, c)| c.name() == tgt_col.name)
                    {
                        field_to_cell(&src_row, i)?
                    } else {
                        None
                    };
                    row.push(&tgt_col.name, cell);
                }
                self.row_idx += 1;
                return Ok(Some(()));
            }
        }
        Ok(None)
    }

    fn end_scan(&mut self) -> ClickHouseFdwResult<()> {
        self.scan_blk.take();
        Ok(())
    }

    fn begin_modify(&mut self, options: &HashMap<String, String>) -> ClickHouseFdwResult<()> {
        self.create_client()?;

        self.table = require_option("table", options)?.to_string();
        self.rowid_col = require_option("rowid_column", options)?.to_string();
        Ok(())
    }

    fn insert(&mut self, src: &Row) -> ClickHouseFdwResult<()> {
        if let Some(ref mut client) = self.client {
            // use a dummy query to probe column types
            let sql = format!("select * from {} where false", self.table);
            let probe = self.rt.block_on(client.query(&sql).fetch_all())?;

            // add row to block
            let mut row = Vec::new();
            for (col_name, cell) in src.iter() {
                let col_name = col_name.to_owned();
                let tgt_col = probe.get_column(col_name.as_ref())?;
                let tgt_type = tgt_col.sql_type();
                let is_nullable = matches!(tgt_type, SqlType::Nullable(_));

                let value = cell
                    .as_ref()
                    .map(|c| match c {
                        Cell::Bool(v) => {
                            let val = if is_nullable {
                                ChValue::from(Some(*v))
                            } else {
                                ChValue::from(*v)
                            };
                            Ok(val)
                        }
                        Cell::I8(v) => {
                            let val = if is_nullable {
                                ChValue::from(Some(*v))
                            } else {
                                ChValue::from(*v)
                            };
                            Ok(val)
                        }
                        Cell::I16(v) => match tgt_col.sql_type() {
                            // i16 can be converted to 2 ClickHouse types: Int16 and UInt8
                            SqlType::Int16 | SqlType::Nullable(SqlType::Int16) => {
                                let val = if is_nullable {
                                    ChValue::from(Some(*v))
                                } else {
                                    ChValue::from(*v)
                                };
                                Ok(val)
                            }
                            SqlType::UInt8 | SqlType::Nullable(SqlType::UInt8) => {
                                let val = if is_nullable {
                                    ChValue::from(Some(*v as u8))
                                } else {
                                    ChValue::from(*v as u8)
                                };
                                Ok(val)
                            }
                            _ => Err(ClickHouseFdwError::UnsupportedColumnType(
                                tgt_type.to_string().into(),
                            )),
                        },
                        Cell::F32(v) => {
                            let val = if is_nullable {
                                ChValue::from(Some(*v))
                            } else {
                                ChValue::from(*v)
                            };
                            Ok(val)
                        }
                        Cell::I32(v) => match tgt_col.sql_type() {
                            // i32 can be converted to 2 ClickHouse types: Int32 and UInt16
                            SqlType::Int32 | SqlType::Nullable(SqlType::Int32) => {
                                let val = if is_nullable {
                                    ChValue::from(Some(*v))
                                } else {
                                    ChValue::from(*v)
                                };
                                Ok(val)
                            }
                            SqlType::UInt16 | SqlType::Nullable(SqlType::UInt16) => {
                                let val = if is_nullable {
                                    ChValue::from(Some(*v as u16))
                                } else {
                                    ChValue::from(*v as u16)
                                };
                                Ok(val)
                            }
                            _ => Err(ClickHouseFdwError::UnsupportedColumnType(
                                tgt_type.to_string().into(),
                            )),
                        },
                        Cell::F64(v) => {
                            let val = if is_nullable {
                                ChValue::from(Some(*v))
                            } else {
                                ChValue::from(*v)
                            };
                            Ok(val)
                        }
                        Cell::I64(v) => match tgt_col.sql_type() {
                            // i64 can be converted to 2 ClickHouse types: Int64 and UInt32
                            SqlType::Int64 | SqlType::Nullable(SqlType::Int64) => {
                                let val = if is_nullable {
                                    ChValue::from(Some(*v))
                                } else {
                                    ChValue::from(*v)
                                };
                                Ok(val)
                            }
                            SqlType::UInt32 | SqlType::Nullable(SqlType::UInt32) => {
                                let val = if is_nullable {
                                    ChValue::from(Some(*v as u32))
                                } else {
                                    ChValue::from(*v as u32)
                                };
                                Ok(val)
                            }
                            _ => Err(ClickHouseFdwError::UnsupportedColumnType(
                                tgt_type.to_string().into(),
                            )),
                        },
                        Cell::Numeric(v) => {
                            let v = types::Decimal::from_str(v.normalize())?;
                            let val = if is_nullable {
                                ChValue::from(Some(v))
                            } else {
                                ChValue::from(v)
                            };
                            Ok(val)
                        }
                        Cell::String(v) => {
                            let s = v.as_str();

                            // i256 and u256 are saved as string in Postgres, so we parse it
                            // back to ClickHouse if target column is Int256 or UInt256
                            let val = match tgt_col.sql_type() {
                                SqlType::Int256 | SqlType::Nullable(SqlType::Int256) => {
                                    let v = i256::from_str(s)?;
                                    if is_nullable {
                                        ChValue::from(Some(v))
                                    } else {
                                        ChValue::from(v)
                                    }
                                }
                                SqlType::UInt256 | SqlType::Nullable(SqlType::UInt256) => {
                                    let v = u256::from_str(s)?;
                                    if is_nullable {
                                        ChValue::from(Some(v))
                                    } else {
                                        ChValue::from(v)
                                    }
                                }
                                _ => {
                                    // other than i256 and u256, convert it to string as normal
                                    if is_nullable {
                                        ChValue::from(Some(s))
                                    } else {
                                        ChValue::from(s)
                                    }
                                }
                            };
                            Ok(val)
                        }
                        Cell::Date(_) => {
                            let s = c.to_string().replace('\'', "");
                            let tm = NaiveDate::parse_from_str(&s, "%Y-%m-%d")?;
                            let val = if is_nullable {
                                ChValue::from(Some(tm))
                            } else {
                                let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
                                let duration = tm - epoch;
                                let dt = duration.num_days() as u16;
                                ChValue::Date(dt)
                            };
                            Ok(val)
                        }
                        Cell::Timestamp(_) => {
                            let s = c.to_string().replace('\'', "");
                            let naive_tm = NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                                .or_else(|_| {
                                NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.6f")
                            })?;
                            let tm: DateTime<Utc> =
                                DateTime::from_naive_utc_and_offset(naive_tm, Utc);
                            let val = if is_nullable {
                                ChValue::Nullable(either::Either::Right(Box::new(tm.into())))
                            } else {
                                ChValue::from(tm)
                            };
                            Ok(val)
                        }
                        Cell::Uuid(v) => {
                            let uuid = Uuid::try_parse(&v.to_string())?;
                            let val = if is_nullable {
                                ChValue::Nullable(either::Either::Right(Box::new(ChValue::Uuid(
                                    *uuid.as_bytes(),
                                ))))
                            } else {
                                ChValue::from(uuid)
                            };
                            Ok(val)
                        }
                        Cell::BoolArray(v) => {
                            array_cell_to_clickhouse_value(v, &SqlType::Bool, is_nullable)
                        }
                        Cell::I16Array(v) => {
                            array_cell_to_clickhouse_value(v, &SqlType::Int16, is_nullable)
                        }
                        Cell::I32Array(v) => {
                            array_cell_to_clickhouse_value(v, &SqlType::Int32, is_nullable)
                        }
                        Cell::I64Array(v) => {
                            array_cell_to_clickhouse_value(v, &SqlType::Int64, is_nullable)
                        }
                        Cell::F32Array(v) => {
                            array_cell_to_clickhouse_value(v, &SqlType::Float32, is_nullable)
                        }
                        Cell::F64Array(v) => {
                            array_cell_to_clickhouse_value(v, &SqlType::Float64, is_nullable)
                        }
                        Cell::StringArray(v) => {
                            array_cell_to_clickhouse_value(v, &SqlType::String, is_nullable)
                        }
                        _ => Err(ClickHouseFdwError::UnsupportedColumnType(
                            tgt_type.to_string().into(),
                        )),
                    })
                    .transpose()?;

                if let Some(v) = value {
                    row.push((col_name, v));
                }
            }
            let mut block = Block::new();
            block.push(row)?;

            // execute query on ClickHouse
            self.rt.block_on(client.insert(&self.table, block))?;
        }
        Ok(())
    }

    fn update(&mut self, rowid: &Cell, new_row: &Row) -> ClickHouseFdwResult<()> {
        if let Some(ref mut client) = self.client {
            let mut sets = Vec::new();
            for (col, cell) in new_row.iter() {
                if col == &self.rowid_col {
                    continue;
                }
                if let Some(cell) = cell {
                    match cell {
                        Cell::Uuid(_) => sets.push(format!("{} = '{}'", col, cell)),
                        _ => sets.push(format!("{} = {}", col, cell)),
                    }
                } else {
                    sets.push(format!("{} = null", col));
                }
            }
            let sql = format!(
                "alter table {} update {} where {} = {}",
                self.table,
                sets.join(", "),
                self.rowid_col,
                rowid
            );

            // execute query on ClickHouse
            self.rt.block_on(client.execute(&sql))?;
        }
        Ok(())
    }

    fn delete(&mut self, rowid: &Cell) -> ClickHouseFdwResult<()> {
        if let Some(ref mut client) = self.client {
            let sql = format!(
                "alter table {} delete where {} = {}",
                self.table, self.rowid_col, rowid
            );

            // execute query on ClickHouse
            self.rt.block_on(client.execute(&sql))?;
        }
        Ok(())
    }
}
