//! One day's records as a Parquet file: decimals as `Decimal128(38, s)`, times as UTC nanoseconds.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use arrow_array::builder::{ListBuilder, StringBuilder};
use arrow_array::cast::AsArray;
use arrow_array::types::{Decimal128Type, TimestampNanosecondType, UInt64Type};
use arrow_array::{
    Array, ArrayRef, Decimal128Array, RecordBatch, StringArray, TimestampNanosecondArray,
    UInt64Array,
};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use mandate_canon::DecStr;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use super::{BAR_SCALE, DatasetError, PRICE_SCALE, SIZE_SCALE};
use crate::model::{Bar, DatasetId, Kind, Quote, Records, Trade};
use crate::number::{self, PRECISION};
use crate::timestamp;

const UTC: &str = "UTC";

fn decimal(scale: u8) -> DataType {
    DataType::Decimal128(PRECISION, i8::try_from(scale).unwrap_or(i8::MAX))
}

fn time() -> DataType {
    DataType::Timestamp(TimeUnit::Nanosecond, Some(UTC.into()))
}

fn conditions() -> DataType {
    DataType::List(Arc::new(Field::new_list_field(DataType::Utf8, true)))
}

/// The partition schema of `kind`; `symbol` comes first and the record time second.
pub fn schema(kind: Kind) -> SchemaRef {
    let fields = match kind {
        Kind::Bars(_) => vec![
            Field::new("symbol", DataType::Utf8, false),
            Field::new("start", time(), false),
            Field::new("open", decimal(BAR_SCALE), false),
            Field::new("high", decimal(BAR_SCALE), false),
            Field::new("low", decimal(BAR_SCALE), false),
            Field::new("close", decimal(BAR_SCALE), false),
            Field::new("volume", decimal(BAR_SCALE), false),
            Field::new("vwap", decimal(BAR_SCALE), false),
            Field::new("trade_count", DataType::UInt64, false),
        ],
        Kind::Trades => vec![
            Field::new("symbol", DataType::Utf8, false),
            Field::new("time", time(), false),
            Field::new("price", decimal(PRICE_SCALE), false),
            Field::new("size", decimal(SIZE_SCALE), false),
            Field::new("trade_id", DataType::UInt64, false),
            Field::new("exchange", DataType::Utf8, true),
            Field::new("conditions", conditions(), true),
            Field::new("tape", DataType::Utf8, true),
            Field::new("taker_side", DataType::Utf8, true),
        ],
        Kind::Quotes => vec![
            Field::new("symbol", DataType::Utf8, false),
            Field::new("time", time(), false),
            Field::new("bid_price", decimal(PRICE_SCALE), false),
            Field::new("bid_size", decimal(SIZE_SCALE), false),
            Field::new("ask_price", decimal(PRICE_SCALE), false),
            Field::new("ask_size", decimal(SIZE_SCALE), false),
            Field::new("bid_exchange", DataType::Utf8, true),
            Field::new("ask_exchange", DataType::Utf8, true),
            Field::new("conditions", conditions(), true),
            Field::new("tape", DataType::Utf8, true),
        ],
    };
    Arc::new(Schema::new(fields))
}

fn parquet_error(e: impl std::fmt::Display) -> DatasetError {
    DatasetError::Parquet(e.to_string())
}

fn decimals<T>(
    rows: &[T],
    column: &'static str,
    scale: u8,
    value: impl Fn(&T) -> &DecStr,
) -> Result<ArrayRef, DatasetError> {
    let units = rows
        .iter()
        .enumerate()
        .map(|(row, r)| {
            number::to_units(value(r), scale).map_err(|source| DatasetError::Number {
                column,
                row,
                source,
            })
        })
        .collect::<Result<Vec<i128>, _>>()?;
    let array = Decimal128Array::from(units)
        .with_precision_and_scale(PRECISION, i8::try_from(scale).map_err(parquet_error)?)
        .map_err(parquet_error)?;
    Ok(Arc::new(array))
}

fn times<T>(
    rows: &[T],
    value: impl Fn(&T) -> mandate_time::UtcNanos,
) -> Result<ArrayRef, DatasetError> {
    let nanos = rows
        .iter()
        .enumerate()
        .map(|(row, r)| {
            timestamp::to_unix_nanos(value(r)).map_err(|source| DatasetError::Time { row, source })
        })
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(Arc::new(
        TimestampNanosecondArray::from(nanos).with_timezone(UTC),
    ))
}

fn strings<'a, T>(rows: &'a [T], value: impl Fn(&'a T) -> Option<&'a str>) -> ArrayRef {
    Arc::new(rows.iter().map(value).collect::<StringArray>())
}

fn bar_columns(symbol: &str, bars: &[Bar]) -> Result<Vec<ArrayRef>, DatasetError> {
    Ok(vec![
        strings(bars, |_| Some(symbol)),
        times(bars, |b| b.start)?,
        decimals(bars, "open", BAR_SCALE, |b| &b.open)?,
        decimals(bars, "high", BAR_SCALE, |b| &b.high)?,
        decimals(bars, "low", BAR_SCALE, |b| &b.low)?,
        decimals(bars, "close", BAR_SCALE, |b| &b.close)?,
        decimals(bars, "volume", BAR_SCALE, |b| &b.volume)?,
        decimals(bars, "vwap", BAR_SCALE, |b| &b.vwap)?,
        Arc::new(bars.iter().map(|b| b.trade_count).collect::<UInt64Array>()),
    ])
}

fn condition_lists<T>(rows: &[T], value: impl Fn(&T) -> Option<&[String]>) -> ArrayRef {
    let mut lists = ListBuilder::new(StringBuilder::new());
    for row in rows {
        match value(row) {
            Some(list) => {
                for condition in list {
                    lists.values().append_value(condition);
                }
                lists.append(true);
            }
            None => lists.append(false),
        }
    }
    Arc::new(lists.finish())
}

fn trade_columns(symbol: &str, trades: &[Trade]) -> Result<Vec<ArrayRef>, DatasetError> {
    Ok(vec![
        strings(trades, |_| Some(symbol)),
        times(trades, |t| t.time)?,
        decimals(trades, "price", PRICE_SCALE, |t| &t.price)?,
        decimals(trades, "size", SIZE_SCALE, |t| &t.size)?,
        Arc::new(trades.iter().map(|t| t.trade_id).collect::<UInt64Array>()),
        strings(trades, |t| t.exchange.as_deref()),
        condition_lists(trades, |t| t.conditions.as_deref()),
        strings(trades, |t| t.tape.as_deref()),
        strings(trades, |t| t.taker_side.as_deref()),
    ])
}

fn quote_columns(symbol: &str, quotes: &[Quote]) -> Result<Vec<ArrayRef>, DatasetError> {
    Ok(vec![
        strings(quotes, |_| Some(symbol)),
        times(quotes, |q| q.time)?,
        decimals(quotes, "bid_price", PRICE_SCALE, |q| &q.bid_price)?,
        decimals(quotes, "bid_size", SIZE_SCALE, |q| &q.bid_size)?,
        decimals(quotes, "ask_price", PRICE_SCALE, |q| &q.ask_price)?,
        decimals(quotes, "ask_size", SIZE_SCALE, |q| &q.ask_size)?,
        strings(quotes, |q| q.bid_exchange.as_deref()),
        strings(quotes, |q| q.ask_exchange.as_deref()),
        condition_lists(quotes, |q| q.conditions.as_deref()),
        strings(quotes, |q| q.tape.as_deref()),
    ])
}

/// One day's records as Parquet bytes. The same records always give the same bytes.
pub fn encode(dataset: &DatasetId, records: &Records) -> Result<Vec<u8>, DatasetError> {
    let symbol = dataset.symbol().as_str();
    let columns = match records {
        Records::Bars(bars) => bar_columns(symbol, bars)?,
        Records::Trades(trades) => trade_columns(symbol, trades)?,
        Records::Quotes(quotes) => quote_columns(symbol, quotes)?,
    };
    let schema = schema(dataset.kind());
    let batch = RecordBatch::try_new(Arc::clone(&schema), columns).map_err(parquet_error)?;
    let properties = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut bytes = Vec::new();
    let mut writer =
        ArrowWriter::try_new(&mut bytes, schema, Some(properties)).map_err(parquet_error)?;
    writer.write(&batch).map_err(parquet_error)?;
    writer.close().map_err(parquet_error)?;
    Ok(bytes)
}

struct Columns<'a> {
    batch: &'a RecordBatch,
    offset: usize,
}

impl Columns<'_> {
    fn column(&self, name: &str) -> Result<&ArrayRef, DatasetError> {
        self.batch
            .column_by_name(name)
            .ok_or_else(|| DatasetError::Parquet(format!("no column `{name}`")))
    }

    fn no_nulls(&self, name: &str, array: &dyn Array) -> Result<(), DatasetError> {
        if array.null_count() > 0 {
            return Err(DatasetError::Parquet(format!("column `{name}` has nulls")));
        }
        Ok(())
    }

    fn decimals(&self, name: &'static str) -> Result<Vec<DecStr>, DatasetError> {
        let array = self
            .column(name)?
            .as_primitive_opt::<Decimal128Type>()
            .ok_or_else(|| DatasetError::Parquet(format!("column `{name}` is not decimal")))?;
        self.no_nulls(name, array)?;
        let scale = u8::try_from(array.scale()).map_err(parquet_error)?;
        array
            .values()
            .iter()
            .enumerate()
            .map(|(i, units)| {
                number::from_units(*units, scale).map_err(|source| DatasetError::Number {
                    column: name,
                    row: self.offset.saturating_add(i),
                    source,
                })
            })
            .collect()
    }

    fn times(&self, name: &str) -> Result<Vec<mandate_time::UtcNanos>, DatasetError> {
        let array = self
            .column(name)?
            .as_primitive_opt::<TimestampNanosecondType>()
            .ok_or_else(|| DatasetError::Parquet(format!("column `{name}` is not a timestamp")))?;
        self.no_nulls(name, array)?;
        array
            .values()
            .iter()
            .enumerate()
            .map(|(i, nanos)| {
                timestamp::from_unix_nanos(*nanos).map_err(|source| DatasetError::Time {
                    row: self.offset.saturating_add(i),
                    source,
                })
            })
            .collect()
    }

    fn counts(&self, name: &str) -> Result<Vec<u64>, DatasetError> {
        let array = self
            .column(name)?
            .as_primitive_opt::<UInt64Type>()
            .ok_or_else(|| DatasetError::Parquet(format!("column `{name}` is not UInt64")))?;
        self.no_nulls(name, array)?;
        Ok(array.values().to_vec())
    }

    fn strings(&self, name: &str) -> Result<Vec<Option<String>>, DatasetError> {
        let array = self
            .column(name)?
            .as_string_opt::<i32>()
            .ok_or_else(|| DatasetError::Parquet(format!("column `{name}` is not Utf8")))?;
        Ok(array.iter().map(|s| s.map(str::to_owned)).collect())
    }

    fn lists(&self, name: &str) -> Result<Vec<Option<Vec<String>>>, DatasetError> {
        let array = self
            .column(name)?
            .as_list_opt::<i32>()
            .ok_or_else(|| DatasetError::Parquet(format!("column `{name}` is not a list")))?;
        array
            .iter()
            .map(|item| {
                item.map(|values| {
                    let values = values.as_string_opt::<i32>().ok_or_else(|| {
                        DatasetError::Parquet(format!("column `{name}` holds non-strings"))
                    })?;
                    values
                        .iter()
                        .map(|v| {
                            v.map(str::to_owned).ok_or_else(|| {
                                DatasetError::Parquet(format!("column `{name}` holds a null"))
                            })
                        })
                        .collect()
                })
                .transpose()
            })
            .collect()
    }
}

fn read_bars(c: &Columns<'_>, out: &mut Vec<Bar>) -> Result<(), DatasetError> {
    let start = c.times("start")?;
    let open = c.decimals("open")?;
    let high = c.decimals("high")?;
    let low = c.decimals("low")?;
    let close = c.decimals("close")?;
    let volume = c.decimals("volume")?;
    let vwap = c.decimals("vwap")?;
    let trade_count = c.counts("trade_count")?;
    let rows = start
        .into_iter()
        .zip(open)
        .zip(high)
        .zip(low)
        .zip(close)
        .zip(volume)
        .zip(vwap)
        .zip(trade_count);
    for (((((((start, open), high), low), close), volume), vwap), trade_count) in rows {
        out.push(Bar {
            start,
            open,
            high,
            low,
            close,
            volume,
            vwap,
            trade_count,
        });
    }
    Ok(())
}

fn read_trades(c: &Columns<'_>, out: &mut Vec<Trade>) -> Result<(), DatasetError> {
    let time = c.times("time")?;
    let price = c.decimals("price")?;
    let size = c.decimals("size")?;
    let trade_id = c.counts("trade_id")?;
    let exchange = c.strings("exchange")?;
    let conditions = c.lists("conditions")?;
    let tape = c.strings("tape")?;
    let taker_side = c.strings("taker_side")?;
    let rows = time
        .into_iter()
        .zip(price)
        .zip(size)
        .zip(trade_id)
        .zip(exchange)
        .zip(conditions)
        .zip(tape)
        .zip(taker_side);
    for (((((((time, price), size), trade_id), exchange), conditions), tape), taker_side) in rows {
        out.push(Trade {
            time,
            price,
            size,
            trade_id,
            exchange,
            conditions,
            tape,
            taker_side,
        });
    }
    Ok(())
}

fn read_quotes(c: &Columns<'_>, out: &mut Vec<Quote>) -> Result<(), DatasetError> {
    let time = c.times("time")?;
    let bid_price = c.decimals("bid_price")?;
    let bid_size = c.decimals("bid_size")?;
    let ask_price = c.decimals("ask_price")?;
    let ask_size = c.decimals("ask_size")?;
    let bid_exchange = c.strings("bid_exchange")?;
    let ask_exchange = c.strings("ask_exchange")?;
    let conditions = c.lists("conditions")?;
    let tape = c.strings("tape")?;
    let rows = time
        .into_iter()
        .zip(bid_price)
        .zip(bid_size)
        .zip(ask_price)
        .zip(ask_size)
        .zip(bid_exchange)
        .zip(ask_exchange)
        .zip(conditions)
        .zip(tape);
    for (
        (
            ((((((time, bid_price), bid_size), ask_price), ask_size), bid_exchange), ask_exchange),
            conditions,
        ),
        tape,
    ) in rows
    {
        out.push(Quote {
            time,
            bid_price,
            bid_size,
            ask_price,
            ask_size,
            bid_exchange,
            ask_exchange,
            conditions,
            tape,
        });
    }
    Ok(())
}

/// Reads a partition written by [`encode`]; a file with another schema is an error.
pub fn read(path: &Path, kind: Kind) -> Result<Records, DatasetError> {
    let file = File::open(path).map_err(|source| DatasetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(parquet_error)?;
    if builder.schema().fields() != schema(kind).fields() {
        return Err(DatasetError::Parquet(format!(
            "{} does not have the {} schema",
            path.display(),
            kind.dir_name()
        )));
    }
    let reader = builder.build().map_err(parquet_error)?;
    let mut records = Records::empty(kind);
    let mut offset = 0_usize;
    for batch in reader {
        let batch = batch.map_err(parquet_error)?;
        let columns = Columns {
            batch: &batch,
            offset,
        };
        match &mut records {
            Records::Bars(bars) => read_bars(&columns, bars)?,
            Records::Trades(trades) => read_trades(&columns, trades)?,
            Records::Quotes(quotes) => read_quotes(&columns, quotes)?,
        }
        offset = offset.saturating_add(batch.num_rows());
    }
    Ok(records)
}
