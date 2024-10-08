use anyhow::{anyhow, Result};
use std::collections::HashMap;

use nom::{
    branch::alt,
    bytes::complete::{tag, take_until},
    IResult,
};

fn parse_timeseries(input: &str) -> IResult<&str, &str> {
    alt((tag("TIMESERIES"), tag("TABLE")))(input)
}

fn parse_limit(input: &str) -> IResult<&str, &str> {
    let (remainder, _) = tag("LIMIT")(input)?;
    alt((take_until("TIMESERIES"), take_until("TABLE")))(remainder)
}

fn parse_until(input: &str) -> IResult<&str, &str> {
    let (remainder, _) = tag("UNTIL")(input)?;
    take_until("LIMIT")(remainder)
}

fn parse_since(input: &str) -> IResult<&str, &str> {
    let (remainder, _) = tag("SINCE")(input)?;
    take_until("UNTIL")(remainder)
}

fn parse_facet(input: &str) -> IResult<&str, &str> {
    let (remainder, _) = tag("FACET")(input)?;
    take_until("SINCE")(remainder)
}

fn parse_where(input: &str) -> IResult<&str, &str> {
    let (remainder, _) = tag("WHERE")(input)?;
    alt((take_until("FACET"), take_until("SINCE")))(remainder)
}

fn parse_select(input: &str) -> IResult<&str, &str> {
    let (remainder, _) = tag("SELECT")(input)?;
    take_until("WHERE")(remainder)
}

fn parse_from(input: &str) -> IResult<&str, &str> {
    let (remainder, _) = tag("FROM")(input)?;
    take_until("SELECT")(remainder)
}

// TODO: Handle missing components
pub fn parse_nrql(input: &str) -> Result<HashMap<String, String>> {
    let mut res = String::new();

    let (remainder, from) = parse_from(input).map_err(|_| anyhow!("Parsing Error! : FROM"))?;
    let (remainder, select) =
        parse_select(remainder).map_err(|_| anyhow!("Parsing Error!: SELECT"))?;
    let (remainder, r#where) =
        parse_where(remainder).map_err(|_| anyhow!("Parsing Error! : WHERE"))?;
    let (remainder, facet) = parse_facet(remainder).unwrap_or((remainder, ""));
    let (remainder, since) =
        parse_since(remainder).map_err(|_| anyhow!("Parsing Error! : SINCE"))?;
    let (remainder, until) =
        parse_until(remainder).map_err(|_| anyhow!("Parsing Error! : UNTIL"))?;
    let (remainder, limit) =
        parse_limit(remainder).map_err(|_| anyhow!("Parsing Error! : LIMIT"))?;
    let (_, mode) = parse_timeseries(remainder).map_err(|_| anyhow!("Parsing error! : MODE"))?;

    let mut outputs = HashMap::new();

    outputs.insert("FROM".to_owned(), from.trim().to_owned());
    outputs.insert("SELECT".to_owned(), select.trim().to_owned());
    outputs.insert("WHERE".to_owned(), r#where.trim().to_owned());
    outputs.insert("FACET".to_owned(), facet.trim().to_owned());
    outputs.insert("SINCE".to_owned(), since.trim().to_owned());
    outputs.insert("UNTIL".to_owned(), until.trim().to_owned());
    outputs.insert("LIMIT".to_owned(), limit.trim().to_owned());
    outputs.insert("MODE".to_owned(), mode.trim().to_owned());

    Ok(outputs)
}
