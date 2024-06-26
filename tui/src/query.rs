use std::ops::Add;

use anyhow::Result;
use serde::Deserialize;

use crate::parser::parse_nrql;

#[derive(Default, Debug, Deserialize, Clone)]
pub enum QueryType {
    #[default]
    Timeseries,
}

pub struct NRQLResult {}

#[derive(Default, Debug, Deserialize, Clone)]
pub struct NRQLQuery {
    pub from: String,
    pub select: String,
    pub r#where: String,
    pub facet: String,
    pub since: String,
    pub until: String,
    pub limit: String,
    pub mode: String,
}

impl NRQLQuery {
    pub fn to_string(&self) -> Result<String> {
        let mut query = String::new();
        query += format!("FROM {} ", self.from).as_str();
        query += format!("SELECT {} as value ", self.select).as_str();
        query += format!("WHERE {} ", self.r#where).as_str();
        if !String::is_empty(&self.facet) {
            query += format!("FACET {} ", self.facet).as_str();
        }
        query += format!("SINCE {} ", self.since).as_str();
        query += format!("UNTIL {} ", self.until).as_str();
        query += format!("LIMIT {} ", self.limit).as_str();
        query += format!("{}", self.mode).as_str();

        Ok(query.to_string())
    }
}

impl NRQL for &str {
    fn to_nrql(self) -> Result<NRQLQuery> {
        let parts = parse_nrql(self)?;
        let mut nrql = NRQLQuery::default();
        parts.iter().for_each(|(key, value)| match key.as_ref() {
            "FROM" => nrql.from = value.to_owned(),
            "SELECT" => nrql.select = value.to_owned(),
            "WHERE" => nrql.r#where = value.to_owned(),
            "FACET" => nrql.facet = value.to_owned(),
            "SINCE" => nrql.since = value.to_owned(),
            "UNTIL" => nrql.until = value.to_owned(),
            "LIMIT" => nrql.limit = value.to_owned(),
            "MODE" => nrql.mode = value.to_owned(),
            _ => panic!(),
        });
        Ok(nrql)
    }
}

pub trait NRQL {
    fn to_nrql(self) -> Result<NRQLQuery>;
}
