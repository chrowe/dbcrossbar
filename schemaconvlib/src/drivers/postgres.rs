//! Driver for working with PostgreSQL schemas.

// See https://github.com/diesel-rs/diesel/issues/1785
#![allow(missing_docs, proc_macro_derive_resolution_fallback)]

use diesel::{pg::PgConnection, prelude::*};
use failure::ResultExt;
use std::io::Write;
use url::Url;

use Result;
use table::{Column, DataType, Table};

table! {
    // https://www.postgresql.org/docs/10/static/infoschema-columns.html
    information_schema.columns (table_catalog, table_schema, table_name, column_name) {
        table_catalog -> VarChar,
        table_schema -> VarChar,
        table_name -> VarChar,
        column_name -> VarChar,
        ordinal_position -> Integer,
        is_nullable -> VarChar,
        data_type -> VarChar,
        udt_schema -> VarChar,
        udt_name -> VarChar,
    }
}

#[derive(Queryable, Insertable)]
#[table_name = "columns"]
struct PgColumn {
    table_catalog: String,
    table_schema: String,
    table_name: String,
    column_name: String,
    ordinal_position: i32,
    is_nullable: String,
    data_type: String,
    udt_schema: String,
    udt_name: String,
}

impl PgColumn {
    /// Get the data type for a column.
    fn data_type(&self) -> Result<DataType> {
        pg_data_type(&self.data_type, &self.udt_schema, &self.udt_name)
    }
}

/// A driver for working with PostgreSQL.
pub struct PostgresDriver;

impl PostgresDriver {
    /// Fetch information about a table from the database.
    pub fn fetch_from_url(
        database_url: &Url,
        full_table_name: &str,
    ) -> Result<Table> {
        let conn = PgConnection::establish(database_url.as_str())
            .context("error connecting to PostgreSQL")?;
        let (table_schema, table_name) = parse_full_table_name(full_table_name);
        let pg_columns = columns::table
            .filter(columns::table_schema.eq(table_schema))
            .filter(columns::table_name.eq(table_name))
            .order(columns::ordinal_position)
            .load::<PgColumn>(&conn)?;

        let mut columns = Vec::with_capacity(pg_columns.len());
        for pg_col in pg_columns {
            let data_type = pg_col.data_type()?;
            columns.push(Column {
                name: pg_col.column_name,
                data_type,
                is_nullable: match pg_col.is_nullable.as_str() {
                    "YES" => true,
                    "NO" => false,
                    value => {
                        return Err(format_err!(
                            "Unexpected is_nullable value: {:?}", value,
                        ))
                    }
                },
                comment: None,
            })
        }

        Ok(Table { name: table_name.to_owned(), columns })
    }

    /// Write out a table's column names as `SELECT` arguments.
    pub fn write_select_args(f: &mut Write, table: &Table) -> Result<()> {
        let mut first: bool = true;
        for col in &table.columns {
            if first {
                first = false;
            } else {
                write!(f, ",")?;
            }
            write!(f, "{:?}", col.name)?;
        }
        Ok(())
    }
}

/// Given a name of the form `mytable` or `myschema.mytable`, split it into
/// a `table_schema` and `table_name`.
fn parse_full_table_name(full_table_name: &str) -> (&str, &str) {
    if let Some(pos) = full_table_name.find('.') {
        (&full_table_name[..pos], &full_table_name[pos+1..])
    } else {
        ("public", full_table_name)
    }
}

#[test]
fn parsing_full_table_name() {
    assert_eq!(parse_full_table_name("mytable"), ("public", "mytable"));
    assert_eq!(parse_full_table_name("other.mytable"), ("other", "mytable"));
}

/// Choose an appropriate `DataType`.
fn pg_data_type(
    data_type: &str,
    _udt_schema: &str,
    udt_name: &str,
) -> Result<DataType> {
    if data_type == "ARRAY" {
        // Array element types have their own naming convention, which appears
        // to be "_" followed by the internal udt_name version of PostgreSQL's
        // base types.
        let element_type = match udt_name {
            "_bool" => DataType::Boolean,
            "_float8" => DataType::DoublePrecision,
            "_int4" => DataType::Integer,
            "_text" => DataType::Text,
            "_uuid" => DataType::Uuid,
            _ => return Err(format_err!("unknown array element {:?}", udt_name)),
        };
        Ok(DataType::Array(Box::new(element_type)))
    } else if data_type == "USER-DEFINED" {
        Ok(DataType::Other(udt_name.to_owned()))
    } else {
        data_type.parse()
    }
}

#[test]
fn parsing_pg_data_type() {
    let examples = &[
        // Basic types.
        (("bigint", "pg_catalog", "int8"),
         DataType::Bigint),
        (("boolean", "pg_catalog", "bool"),
         DataType::Boolean),
        (("character varying", "pg_catalog", "varchar"),
         DataType::CharacterVarying),
        (("date", "pg_catalog", "date"),
         DataType::Date),
        (("double precision", "pg_catalog", "float8"),
         DataType::DoublePrecision),
        (("integer", "pg_catalog", "int4"),
         DataType::Integer),
        (("interval", "pg_catalog", "interval"),
         DataType::Other("interval".to_owned())),
        (("json", "pg_catalog", "json"),
         DataType::Json),
        (("jsonb", "pg_catalog", "jsonb"),
         DataType::Jsonb),
        (("name", "pg_catalog", "name"),
         DataType::Other("name".to_owned())),
        (("numeric", "pg_catalog", "numeric"),
         DataType::Numeric),
        (("oid", "pg_catalog", "oid"),
         DataType::Other("oid".to_owned())),
        (("real", "pg_catalog", "float4"),
         DataType::Real),
        (("regclass", "pg_catalog", "regclass"),
         DataType::Other("regclass".to_owned())),
        (("regtype", "pg_catalog", "regtype"),
         DataType::Other("regtype".to_owned())),
        (("smallint", "pg_catalog", "int2"),
         DataType::Smallint),
        (("text", "pg_catalog", "text"),
         DataType::Text),
        (("timestamp without time zone", "pg_catalog", "timestamp"),
         DataType::TimestampWithoutTimeZone),

        // Array types.
        (("ARRAY", "pg_catalog", "_bool"),
         DataType::Array(Box::new(DataType::Boolean))),
        (("ARRAY", "pg_catalog", "_float8"),
         DataType::Array(Box::new(DataType::DoublePrecision))),
        (("ARRAY", "pg_catalog", "_int4"),
         DataType::Array(Box::new(DataType::Integer))),
        (("ARRAY", "pg_catalog", "_text"),
         DataType::Array(Box::new(DataType::Text))),
        (("ARRAY", "pg_catalog", "_uuid"),
         DataType::Array(Box::new(DataType::Uuid))),

        // User-defined types.
        (("USER-DEFINED", "public", "citext"),
         DataType::Other("citext".to_owned())),
        (("USER-DEFINED", "public", "geometry"),
         DataType::Other("geometry".to_owned())),
    ];
    for ((data_type, udt_schema, udt_name), expected) in examples {
        assert_eq!(
            &pg_data_type(data_type, udt_schema, udt_name).unwrap(),
            expected,
        );
    }
}
