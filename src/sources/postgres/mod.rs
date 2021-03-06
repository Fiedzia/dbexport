use std::fs::File;
use std::io::Read;

use fallible_iterator::FallibleIterator;
use postgres::{self, Client, NoTls, types::Kind};
use urlencoding;

use crate::commands::common::PostgresConfigOptions;
use crate::commands::export::PostgresSourceOptions;
use crate::definitions::{ColumnType, Value, Row, ColumnInfo, DataSource, DataSourceConnection, DataSourceBatchIterator};
use crate::utils::report_query_error;


pub trait GetPostgresConnectionParams {
    fn get_hostname(&self) -> &Option<String>;
    fn get_username(&self) -> &Option<String>;
    fn get_password(&self) -> &Option<String>;
    fn get_port(&self) -> &Option<u16>;
    fn get_database(&self) -> &Option<String>;
    fn get_init(&self) -> &Vec<String>;
    fn get_timeout(&self) -> &Option<u64>;
}

impl GetPostgresConnectionParams for PostgresSourceOptions {
    fn get_hostname(&self) -> &Option<String> { &self.host }
    fn get_username(&self) -> &Option<String> { &self.user }
    fn get_password(&self) -> &Option<String> { &self.password }
    fn get_port(&self) -> &Option<u16> { &self.port }
    fn get_database(&self) -> &Option<String> { &self.database }
    fn get_init(&self) -> &Vec<String> { &self.init }
    fn get_timeout(&self) -> &Option<u64> { &self.timeout }
}

impl GetPostgresConnectionParams for PostgresConfigOptions {
    fn get_hostname(&self) -> &Option<String> { &self.host }
    fn get_username(&self) -> &Option<String> { &self.user }
    fn get_password(&self) -> &Option<String> { &self.password }
    fn get_port(&self) -> &Option<u16> { &self.port }
    fn get_database(&self) -> &Option<String> { &self.database }
    fn get_init(&self) -> &Vec<String> { &self.init }
    fn get_timeout(&self) -> &Option<u64> { &self.timeout }
}


pub fn get_postgres_url(postgres_options: &dyn GetPostgresConnectionParams) -> String {
    format!(
        "postgres://{user}{password}{hostname}{port}{database}",
        user=match &postgres_options.get_username() { None => "", Some(v) => v},
        hostname=match &postgres_options.get_hostname() {
            None => "".to_string(),
            Some(v) => format!("@{}", urlencoding::encode(v))
        },
        password=match &postgres_options.get_password() {
            None => "".to_string(),
            Some(p) => format!(":{}", urlencoding::encode(p))
        },
        port=match &postgres_options.get_port() {
            None => "".to_string(),
            Some(p) => format!(":{}", p)
        },
        database=match &postgres_options.get_database() {
            None => "".to_string(),
            Some(d) => format!("/{}", urlencoding::encode(d))
        },
    )
}


pub fn establish_postgres_connection(postgres_options: &dyn GetPostgresConnectionParams) -> Client {

    let database_url = get_postgres_url(postgres_options);
    let mut client = Client::connect(&database_url, NoTls).unwrap();

    if !postgres_options.get_init().is_empty() {
        for sql in postgres_options.get_init().iter() {
            match client.execute(sql.as_str(), &[]) {
                Ok(_) => {},
                Err(e) => {
                    report_query_error(&sql, &format!("{:?}", e));
                    std::process::exit(1);
                }
            }
        }
    }
    client
}



pub struct PostgresSource {
    options: PostgresSourceOptions,
}

pub struct PostgresSourceConnection<'c> {
    connection: Client,
    //results: postgres::RowIter<'c>,//Vec<postgres::row::Row>,
    query: String,
    source: &'c  PostgresSource,
}

pub struct PostgresSourceBatchIterator<'i>
//where 'c: 'i
{
    batch_size: u64,
    result_iterator: postgres::RowIter<'i>, //std::slice::Iter<'i, postgres::row::Row>,
    columns: Vec<postgres::Column>,
    //source_connection: &'i mut PostgresSourceConnection<'c>
}

impl PostgresSource {
    pub fn init(postgres_options: &PostgresSourceOptions) -> PostgresSource {
        PostgresSource { options: postgres_options.to_owned() }
    }
}


impl <'c, 'i> DataSource<'c, 'i, PostgresSourceConnection<'c>, PostgresSourceBatchIterator<'i>> for PostgresSource
where 'c: 'i,
{
    fn connect(&'c self) -> PostgresSourceConnection
    {
        
        let connection =  establish_postgres_connection(&self.options);
        let query = match &self.options.query {
            Some(q) => q.to_owned(),
            None => match &self.options.query_file {
                Some(path_buf) => {
                    let mut sql = String::new();
                    File::open(path_buf).unwrap().read_to_string(&mut sql).unwrap();
                    sql
                },
                None => panic!("You need to pass either q or query-file option"),
            }
        };

        PostgresSourceConnection {
            connection,
            source: &self,
            query: query
            //results,
        }
    }

    fn get_type_name(&self) -> String {"postgres".to_string()}
    fn get_name(&self) -> String { "postgres".to_string() }


}

impl <'c, 'i>DataSourceConnection<'i, PostgresSourceBatchIterator<'i>> for PostgresSourceConnection<'c>
{
    fn batch_iterator(&'i mut self, batch_size: u64) -> PostgresSourceBatchIterator<'i>
    {
         let results = {match self.connection.query_raw(self.query.as_str(), std::iter::empty()) {
            Ok(r) => r,
            Err(e) => {
                report_query_error(&self.query, &format!("{:?}", e));
                std::process::exit(1);
            }
        }};
       
        let columns = vec![];
        /*let columns = match &results.peekable().peek().unwrap() {
            Some(row) => row.columns().iter().map(|c| postgres::Column{name: c.name().to_owned(), type_: c.type_().clone()}).collect(),            None => vec![]
        };*/
        PostgresSourceBatchIterator {
            batch_size,
            //connection: & self.source_connection.connection,
            result_iterator: results,
            columns: columns,
            //source_connection: &mut self,
        }
    }
}

pub fn postgres_to_row(column_info: &[(String,  postgres::types::Type)], postgres_row: &postgres::row::Row) -> Row {
    let mut result = Row::with_capacity(postgres_row.len());
    for (idx, (_name, type_)) in column_info.iter().enumerate() {
        match (type_.kind(), type_.name()) {
            (Kind::Simple, "int4") => result.push(Value::I32( postgres_row.get(idx) )),
            (Kind::Simple, "int8") => result.push(Value::I64( postgres_row.get(idx) )),
            (Kind::Simple, "float4") => result.push(Value::F32( postgres_row.get(idx) )),
            (Kind::Simple, "float8") => result.push(Value::F64( postgres_row.get(idx) )),
            (Kind::Simple, "text") => result.push(Value::String( postgres_row.get(idx) )),
            _ => panic!("postgres: unsupported type: {:?}", type_ )
        }
    }

    result
}


impl <'c, 'i>DataSourceBatchIterator for PostgresSourceBatchIterator<'i>
{
    fn get_column_info(&self) -> Vec<ColumnInfo> {
       let mut result = vec![];
       for column in self.columns.iter() {
            match (column.type_().kind(), column.type_().name()) {
                (Kind::Simple, "int4") => result.push(ColumnInfo{name: column.name().to_string(), data_type: ColumnType::I32}),
                (Kind::Simple, "int8") => result.push(ColumnInfo{name: column.name().to_string(), data_type: ColumnType::I64}),
                (Kind::Simple, "float4") => result.push(ColumnInfo{name: column.name().to_string(), data_type: ColumnType::F32}),
                (Kind::Simple, "float8") => result.push(ColumnInfo{name: column.name().to_string(), data_type: ColumnType::F64}),
                (Kind::Simple, "text") => result.push(ColumnInfo{name: column.name().to_string(), data_type: ColumnType::String}),
                _ => panic!("postgres: unsupported type: {:?}", column.type_() )
            };
       }
       result
    }

    fn get_count(&self) -> Option<u64> {
        self.result_iterator.size_hint().1.map(|v| v as u64)
    }
 
    fn next(&mut self) -> Option<Vec<Row>>
    {
        let rows :Vec<Row> = self.result_iterator
            .by_ref()
            .take(self.batch_size as usize)
            .map(|postgres_row| {
                let mut result = Row::with_capacity(postgres_row.len());
                for (idx, column) in postgres_row.columns().iter().enumerate() {
                    match (column.type_().kind(), column.type_().name()) {
                        (Kind::Simple, "int4") => result.push(Value::I32( postgres_row.get(idx) )),
                        (Kind::Simple, "int8") => result.push(Value::I64( postgres_row.get(idx) )),
                        (Kind::Simple, "float4") => result.push(Value::F32( postgres_row.get(idx) )),
                        (Kind::Simple, "float8") => result.push(Value::F64( postgres_row.get(idx) )),
                        (Kind::Simple, "text") => result.push(Value::String( postgres_row.get(idx) )),
                        _ => panic!("postgres: unsupported type: {:?}", column.type_() )
                    }
                }
                Ok(result)
            }).collect().unwrap();

        if !rows.is_empty() {
            Some(rows)
        } else {
            None
        }
    }
}
