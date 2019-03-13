use std::time::Duration;

use chrono;
use mysql;
use mysql::consts::ColumnType as MyColumnType;
use mysql::consts::ColumnFlags as MyColumnFlags;

use crate::commands::common::MysqlConfigOptions;
use crate::commands::export::MysqlSourceOptions;
use crate::definitions::{ColumnType, Value, Row, ColumnInfo, DataSource, DataSourceConnection, DataSourceBatchIterator};


pub trait GetMysqlConnectionParams {
    fn get_hostname(&self) -> &Option<String>;
    fn get_username(&self) -> &Option<String>;
    fn get_password(&self) -> &Option<String>;
    fn get_port(&self) -> &Option<u16>;
    fn get_socket(&self) -> &Option<String>;
    fn get_database(&self) -> &Option<String>;
    fn get_init(&self) -> &Vec<String>;
    fn get_timeout(&self) -> &Option<u64>;
}

impl GetMysqlConnectionParams for MysqlSourceOptions {
    fn get_hostname(&self) -> &Option<String> { &self.host }
    fn get_username(&self) -> &Option<String> { &self.user }
    fn get_password(&self) -> &Option<String> { &self.password }
    fn get_port(&self) -> &Option<u16> { &self.port }
    fn get_socket(&self) -> &Option<String> { &self.socket }
    fn get_database(&self) -> &Option<String> { &self.database }
    fn get_init(&self) -> &Vec<String> { &self.init }
    fn get_timeout(&self) -> &Option<u64> { &self.timeout }
}

impl GetMysqlConnectionParams for MysqlConfigOptions {
    fn get_hostname(&self) -> &Option<String> { &self.host }
    fn get_username(&self) -> &Option<String> { &self.user }
    fn get_password(&self) -> &Option<String> { &self.password }
    fn get_port(&self) -> &Option<u16> { &self.port }
    fn get_socket(&self) -> &Option<String> { &self.socket }
    fn get_database(&self) -> &Option<String> { &self.database }
    fn get_init(&self) -> &Vec<String> { &self.init }
    fn get_timeout(&self) -> &Option<u64> { &self.timeout }
}

pub fn establish_mysql_connection(mysql_options: &GetMysqlConnectionParams ) -> mysql::Pool {


    let mut option_builder = mysql::OptsBuilder::new();
    option_builder
        .db_name(mysql_options.get_database().to_owned())
        .user(mysql_options.get_username().to_owned())
        .pass(mysql_options.get_password().to_owned());

    if let Some(timeout) = mysql_options.get_timeout() {
         option_builder
            .read_timeout(Some(Duration::from_secs(*timeout)))
            .write_timeout(Some(Duration::from_secs(*timeout)))
            .tcp_connect_timeout(Some(Duration::from_secs(*timeout)));
    };

    if let Some(ref socket) = mysql_options.get_socket() {
        option_builder.socket(Some(socket.to_owned()));
    } else {
        option_builder
            .ip_or_hostname(mysql_options.get_hostname().to_owned().or_else(||Some("localhost".to_string())))
            .tcp_port(mysql_options.get_port().to_owned().unwrap_or(3306));
    };
 
    if !mysql_options.get_init().is_empty() {
        option_builder.init(mysql_options.get_init().to_owned());
    };

    mysql::Pool::new(option_builder).unwrap()
}


pub struct MysqlSource {
    options: MysqlSourceOptions,
}

impl MysqlSource {
    pub fn init(mysql_options: &MysqlSourceOptions) -> MysqlSource {
        MysqlSource { options: mysql_options.to_owned() }
    }
}


pub struct MysqlSourceConnection<'c> {
    connection: mysql::Pool,
    source: &'c MysqlSource,
}

pub struct MysqlSourceBatchIterator<'c, 'i>
where 'c: 'i
{
    batch_size: u64,
    connection: &'i mysql::Pool,
    count: Option<u64>,
    results: mysql::QueryResult<'i>,
    source_connection: &'i MysqlSourceConnection<'c>
}

impl <'c, 'i>MysqlSourceBatchIterator<'c, 'i>
where 'c: 'i {

    pub fn mysql_to_row(column_info: &[ColumnInfo], mysql_row: mysql::Row) -> Row {
        let mut result = Row::with_capacity(mysql_row.len());
        for (idx, value) in mysql_row.unwrap().iter().enumerate() {
            match &value {
                mysql::Value::NULL => result.push(Value::None),
                mysql::Value::Int(v) => result.push(Value::I64(*v)),
                mysql::Value::UInt(v) => result.push(Value::U64(*v)),
                mysql::Value::Float(v) => result.push(Value::F64(*v)),
                mysql::Value::Bytes(v) => match std::str::from_utf8(&v) {
                    Ok(s) => result.push(Value::String(s.to_string())),
                    Err(e) => panic!(format!("mysq: invalid utf8 in '{:?}' for row: {:?} ({})", v, value, e))
                },
                mysql::Value::Date(year, month, day, hour, minute, second, _microsecond) => {
                    match column_info[idx].data_type {
                        ColumnType::Date => result.push(
                            Value::Date(chrono::NaiveDate::from_ymd(i32::from(*year), u32::from(*month), u32::from(*day)))
                        ),
                        ColumnType::DateTime => result.push(
                            Value::DateTime(chrono::NaiveDate::from_ymd(i32::from(*year), u32::from(*month), u32::from(*day)).and_hms(u32::from(*hour), u32::from(*minute), u32::from(*second)))
                        ),
                        ColumnType::Time => result.push(
                            Value::Time(chrono::NaiveTime::from_hms(u32::from(*hour), u32::from(*minute), u32::from(*second)))
                        ),
                        ColumnType::Timestamp => result.push(
                            Value::DateTime(chrono::NaiveDate::from_ymd(i32::from(*year), u32::from(*month), u32::from(*day)).and_hms(u32::from(*hour), u32::from(*minute), u32::from(*second)))
                        ),
                        _ => panic!("mysql: unsupported conversion: {:?} => {:?}", value, column_info[idx])
                    }
                },
                //TODO: what to do with negative?
                mysql::Value::Time(_negative, _day, hour, minute, second, _microsecond) => {
                    match column_info[idx].data_type {
                        ColumnType::Time => result.push(
                            Value::Time(chrono::NaiveTime::from_hms(u32::from(*hour), u32::from(*minute), u32::from(*second)))
                        ),
                        _ => panic!("mysql: unsupported conversion: {:?} => {:?}", value, column_info[idx])
                    }
                },
            }
        }
        result
    }

}


impl <'c, 'i> DataSource<'c, 'i, MysqlSourceConnection<'c>, MysqlSourceBatchIterator<'c, 'i>> for MysqlSource
where 'c: 'i,
{
    fn connect(&'c self) -> MysqlSourceConnection
    {

        let connection = establish_mysql_connection(&self.options);

        MysqlSourceConnection {
            connection,
            source: &self,
        }
    }

    fn get_type_name(&self) -> String {"mysql".to_string()}
    fn get_name(&self) -> String { "mysql".to_string() }


}

impl <'c, 'i>DataSourceConnection<'i, MysqlSourceBatchIterator<'c, 'i>> for MysqlSourceConnection<'c>
{
    fn batch_iterator(&'i self, batch_size: u64) -> MysqlSourceBatchIterator<'c, 'i>
    {
        let count: Option<u64> = if self.source.options.count {
            let count_query = format!("select count(*) from ({}) q", self.source.options.query);
            let count_value = self.connection.first_exec(count_query.as_str(), ()).unwrap().unwrap().get(0).unwrap();
            Some(count_value)
        } else {
            None
        };
        let mysql_result = self.connection.prep_exec(self.source.options.query.clone(), ()).unwrap();


        MysqlSourceBatchIterator {
            batch_size,
            connection: &self.connection,
            count,
            results: mysql_result,
            source_connection: &self,
        }
    }
}


impl <'c, 'i>DataSourceBatchIterator for MysqlSourceBatchIterator<'c, 'i>
{
    fn get_column_info(&self) -> Vec<ColumnInfo> {
        let mut result = vec![];
        for column in  self.results.columns_ref() {
            let column_type = column.column_type();
            let flags = column.flags();
            result.push(ColumnInfo {
                name: column.name_str().into_owned(),
                data_type:  match column_type {
                    MyColumnType::MYSQL_TYPE_DECIMAL => ColumnType::Decimal,
                    MyColumnType::MYSQL_TYPE_NEWDECIMAL => ColumnType::Decimal,
                    MyColumnType::MYSQL_TYPE_TINY => 
                        if flags.contains(MyColumnFlags::UNSIGNED_FLAG) {ColumnType::U8} else {ColumnType::I8},
                    MyColumnType::MYSQL_TYPE_SHORT =>
                        if flags.contains(MyColumnFlags::UNSIGNED_FLAG) {ColumnType::U16} else {ColumnType::I16},
                    MyColumnType::MYSQL_TYPE_LONG =>
                        if flags.contains(MyColumnFlags::UNSIGNED_FLAG) {ColumnType::U32} else {ColumnType::I32},
                    MyColumnType::MYSQL_TYPE_LONGLONG =>
                        if flags.contains(MyColumnFlags::UNSIGNED_FLAG) {ColumnType::U64} else {ColumnType::I64},
                    MyColumnType::MYSQL_TYPE_INT24 =>
                        if flags.contains(MyColumnFlags::UNSIGNED_FLAG) {ColumnType::U32} else {ColumnType::I32},
                    MyColumnType::MYSQL_TYPE_VARCHAR
                        | MyColumnType::MYSQL_TYPE_VAR_STRING
                        | MyColumnType::MYSQL_TYPE_STRING => ColumnType::String,
                    MyColumnType::MYSQL_TYPE_FLOAT => ColumnType::F32,
                    MyColumnType::MYSQL_TYPE_DOUBLE => ColumnType::F64,
                    MyColumnType::MYSQL_TYPE_JSON => ColumnType::JSON,
                    MyColumnType::MYSQL_TYPE_TINY_BLOB
                        | MyColumnType::MYSQL_TYPE_MEDIUM_BLOB
                        | MyColumnType::MYSQL_TYPE_LONG_BLOB
                        | MyColumnType::MYSQL_TYPE_BLOB => ColumnType::Bytes,

                    MyColumnType::MYSQL_TYPE_TIMESTAMP => ColumnType::Timestamp,
                    MyColumnType::MYSQL_TYPE_DATE => ColumnType::Date,
                    MyColumnType::MYSQL_TYPE_TIME => ColumnType::Time,
                    MyColumnType::MYSQL_TYPE_TIME2 => ColumnType::Time,
                    MyColumnType::MYSQL_TYPE_DATETIME => ColumnType::DateTime,
                    MyColumnType::MYSQL_TYPE_DATETIME2 => ColumnType::DateTime,
                    MyColumnType::MYSQL_TYPE_YEAR => ColumnType::I64,
                    MyColumnType::MYSQL_TYPE_NEWDATE => ColumnType::Date,
                    MyColumnType::MYSQL_TYPE_TIMESTAMP2 => ColumnType::Timestamp,

                    /*
                    MyColumnType::MYSQL_TYPE_NULL,
                    MyColumnType::MYSQL_TYPE_BIT,
                    MyColumnType::MYSQL_TYPE_ENUM,
                    MyColumnType::MYSQL_TYPE_SET,
                    MyColumnType::MYSQL_TYPE_GEOMETR
                    */
                    _ => panic!(format!("mysql: unsupported column type: {:?}", column_type))
                },
            });
        }
        result


    }

    fn get_count(&self) -> Option<u64> {
        self.count
    }
 
    fn next(&mut self) -> Option<Vec<Row>>
    {
 
        let ci = self.get_column_info();
        let results: Vec<Row> =  self.results
            .by_ref()
            .take(self.batch_size as usize)
            .map(|v|{ MysqlSourceBatchIterator::mysql_to_row(&ci, v.unwrap())})
            .collect();
        match results.len() {
            0 => None,
            _ => Some(results)
        }
    }
}
