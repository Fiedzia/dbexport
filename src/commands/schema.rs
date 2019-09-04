use std::collections::{HashMap, HashSet};

use id_tree::{InsertBehavior, Node, NodeId, Tree};

#[cfg(feature = "use_mysql")]
use mysql;
use regex::RegexBuilder;

use crate::commands::{ApplicationArguments};
use crate::commands::common::{SourceConfigCommandWrapper, SourceConfigCommand};
use crate::utils::report_query_error;

#[cfg(feature = "use_mysql")]
use crate::sources::mysql::{establish_mysql_connection};
#[cfg(feature = "use_postgres")]
use crate::sources::postgres::establish_postgres_connection;
#[cfg(feature = "use_sqlite")]
use crate::sources::sqlite::establish_sqlite_connection;


#[derive(StructOpt)]
pub struct SchemaCommand {
    #[structopt(short = "r", long = "regex", help = "use regular expression engine")]
    pub regex: bool,
    #[structopt(short = "q", long = "query", help = "show items matching query")]
    pub query: Option<String>,
    #[structopt(subcommand)]
    pub source: SourceConfigCommandWrapper,
}


#[derive(Clone, Debug)]
pub struct DBItem {
    name: String,
}

impl DBItem {
    pub fn print(&self, indentation_level: usize) {
        println!("{:indent$}{name}", "", indent=indentation_level * 4, name=self.name);
    }

    pub fn matches(&self, query: &str, is_regex: bool) -> bool {
        if is_regex {
            let re = RegexBuilder::new(query).case_insensitive(true).build().unwrap();
            re.is_match(&self.name)
        } else {
            self.name.to_lowercase().contains(query)
        }
    }

}

#[derive(Clone, Debug)]
struct DBItems(Tree<DBItem>);

impl DBItems {

    pub fn new() -> DBItems {
        DBItems(Tree::new())
    }

    pub fn print(&self) {
        match self.0.root_node_id() {
            None => {},
            Some(root_node_id) => {
                for node_id in self.0.traverse_pre_order_ids(&root_node_id).unwrap() {
                    let node = self.0.get(&node_id).unwrap();
                    if !node.parent().is_none() {
                        let indentation_level = self.0.ancestors(&node_id).unwrap().count() - 1;
                        node.data().print(indentation_level);
                    }
                }
            }
        }
    }

    pub fn subtree_matching_query(&self, query: &str, is_regex:bool) -> DBItems {
        match self.0.root_node_id() {
            None => DBItems::new(),
            Some(root_node_id) => {
                let mut new_dbitems = DBItems::new();
                let mut node_map = HashMap::new();
                for node_id in self.0.traverse_post_order_ids(&root_node_id).unwrap() {
                    if self.0.get(&node_id).unwrap().data().matches(query, is_regex) {
                        let mut ancestor_ids:Vec<NodeId> = self.0
                            .ancestor_ids(&node_id)
                            .unwrap()
                            .map(|v| v.clone())
                            .collect();
                        ancestor_ids.reverse();
                        for node_id in ancestor_ids {
                            let node = self.0.get(&node_id).unwrap();
                            if node_map.contains_key(&node_id) {
                                continue
                            };
                            let new_node_id = new_dbitems.0.insert(
                                Node::new(node.data().clone()),
                                match node.parent() {
                                    None => InsertBehavior::AsRoot,
                                    Some(parent_id) => InsertBehavior::UnderNode(node_map.get(parent_id).unwrap())
                                }
                            ).unwrap();
                            node_map.insert(node_id, new_node_id);
                        }
                        if !node_map.contains_key(&node_id) {

                            let node = self.0.get(&node_id).unwrap();
                            let new_node_id = new_dbitems.0.insert(
                                Node::new(node.data().clone()),
                                match node.parent() {
                                    None => InsertBehavior::AsRoot,
                                    Some(parent_id) => InsertBehavior::UnderNode(node_map.get(parent_id).unwrap())
                                }
                            ).unwrap();
                            node_map.insert(node_id, new_node_id);
                        }
                    }
                }
                new_dbitems
            }
        }
    }
}

pub fn schema (_args: &ApplicationArguments, schema_command: &SchemaCommand) {

    match &schema_command.source.0 {
        #[cfg(feature = "use_mysql")]
        SourceConfigCommand::Mysql(mysql_config_options) => {
            let conn = establish_mysql_connection(mysql_config_options);
            let mut where_parts = vec![];
            let mut params = vec![];
            if let Some(dbname) = &mysql_config_options.database {
                where_parts.push("t.table_schema=?");
                params.push(dbname);
            }
            let where_clause = match where_parts.is_empty() {
                true => "".to_string(),
                false => format!(" where {}", where_parts.iter().map(|v| format!("({})", v) ).collect::<Vec<String>>().join(" AND "))
            };

            let query = format!("
                select
                    t.table_schema, t.table_name,
                    c.column_name, c.column_type
                from
                    information_schema.tables t
                left join
                    information_schema.columns c
                on
                    t.table_schema=c.table_schema
                    and t.table_name=c.table_name
                {}
                order by t.table_schema, t.table_name, c.column_name
                ", where_clause);

            let result = conn.prep_exec(&query, params);
            let results = match result {
                Ok(v) => v,
                Err(e) => {
                    report_query_error(&query, &format!("{:?}", e));
                    std::process::exit(1);
                }
            };
            /*let mut dbitems = DBItems(vec![]);
            for row in results {
                let (schema_name, table_name, column_name, column_type):(String, String, String, String) = mysql::from_row(row.unwrap());
                if dbitems.0.is_empty() {
                    dbitems.0.push( DBItem {name: schema_name.clone(), items: vec![]} );
                } else {
                    if dbitems.0.last().unwrap().name != schema_name {
                        dbitems.0.push( DBItem {name: schema_name.clone(), items: vec![]} );
                    }
                };
                dbitems.0.last_mut().unwrap().items.push(DBItem { name: table_name.clone(), items: vec![]} );
            }
            if let Some(q) = &schema_command.query {
                dbitems = dbitems.subtree_matching_query(&q);
            }
            dbitems.print();*/
        },
        #[cfg(feature = "use_sqlite")]
        SourceConfigCommand::Sqlite(sqlite_config_options) => {
            let conn = establish_sqlite_connection(sqlite_config_options);
            let mut dbitems = DBItems::new();
            let root_node = dbitems.0.insert(
                Node::new(
                    DBItem{name: "".to_string()}
                ),
                InsertBehavior::AsRoot
            ).unwrap();
            let mut current_parent = None;
            conn.iterate("
                SELECT 
                  m.name as table_name, 
                  p.name as name,
                  p.type as type,
                  p.`notnull` as nullability,
                  p.dflt_value as default_value,
                  p.pk as primary_key
                
                FROM 
                  sqlite_master AS m
                JOIN 
                  pragma_table_info(m.name) AS p
                ORDER BY 
                  m.name, 
                  p.cid
                ",
                |row| {
                    let table_name = row[0].1.unwrap();
                    let field_name = row[1].1.unwrap();
                    match &current_parent {
                        None => {
                            current_parent = Some(
                                dbitems.0.insert(
                                    Node::new(
                                        DBItem{name: table_name.to_string()}
                                    ),
                                    InsertBehavior::UnderNode(&root_node)
                                ).unwrap()
                            );
                        },
                        Some(node_id) => {
                            if table_name != dbitems.0.get(node_id).unwrap().data().name {
                                current_parent = Some(
                                    dbitems.0.insert(
                                        Node::new(
                                            DBItem{name: table_name.to_string()}
                                        ),
                                        InsertBehavior::UnderNode(&root_node)
                                    ).unwrap()
                                );
                            }
                        }
                    }
                    dbitems.0.insert(
                        Node::new(
                            DBItem{name: field_name.to_string()}
                        ),
                        InsertBehavior::UnderNode(current_parent.as_ref().unwrap())
                    ).unwrap();
                    true
                }
            ).unwrap();
            if let Some(query) = &schema_command.query {
                dbitems = dbitems.subtree_matching_query(&query.to_lowercase(), schema_command.regex);
            }
            dbitems.print();
        },
        #[cfg(feature = "use_postgres")]
        SourceConfigCommand::Postgres(postgres_config_options) => {
          let _conn = establish_postgres_connection(postgres_config_options);
        }
    }
}
