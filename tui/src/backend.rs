use anyhow::Result;
use std::{
    collections::BTreeMap,
    sync::mpsc::{channel, Receiver, Sender},
    time::Duration,
};
use tokio::{
    runtime::{self, Runtime},
    time::sleep,
};

use chrono::{Timelike, Utc};
use server::{
    timeseries::{Timeseries, TimeseriesResult},
    NewRelicClient,
};

use crate::query::NRQLQuery;

pub struct Bounds {
    pub mins: (f64, f64),
    pub maxes: (f64, f64),
}

pub struct Payload {
    pub query: String,
    pub data: BTreeMap<String, Vec<(f64, f64)>>,
    pub bounds: Bounds,
}

pub struct Backend {
    pub client: NewRelicClient,
    pub runtime: Runtime,
    pub data_tx: Sender<Payload>,
    pub data_rx: Receiver<Payload>,
}

impl Backend {
    pub fn new(client: NewRelicClient) -> Self {
        let (data_tx, data_rx) = channel::<Payload>();
        let runtime = runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("data")
            .enable_all()
            .build()
            .unwrap();

        Self {
            client,
            runtime,
            data_tx,
            data_rx,
        }
    }

    pub fn add_query(&self, query: NRQLQuery) {
        let tx = self.data_tx.clone();
        let client = self.client.clone();
        self.runtime.spawn(async move {
            _ = refresh_timeseries(query, client, tx).await;
        });
    }
}

pub async fn refresh_timeseries(
    query: NRQLQuery,
    client: NewRelicClient,
    data_tx: Sender<Payload>,
) -> Result<()> {
    loop {
        if Utc::now().second() % 5 == 0 {
            let data = client
                .query::<TimeseriesResult>(query.to_string().unwrap())
                .await
                .unwrap_or_default();

            let mut min_bounds: (f64, f64) = (f64::MAX, f64::MAX);
            let mut max_bounds: (f64, f64) = (0 as f64, 0 as f64);

            for point in &data {
                min_bounds.0 = f64::min(min_bounds.0, point.end_time_seconds);
                min_bounds.1 = f64::min(min_bounds.1, point.value);

                max_bounds.0 = f64::max(max_bounds.0, point.end_time_seconds);
                max_bounds.1 = f64::max(max_bounds.1, point.value);
            }

            let mut facets: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::default();

            for data in data.into_iter().map(Timeseries::from) {
                if facets.contains_key(&data.facet) {
                    facets
                        .get_mut(&data.facet)
                        .unwrap()
                        .extend_from_slice(&[(data.end_time_seconds, data.value)]);
                } else {
                    facets.insert(data.facet, vec![(data.begin_time_seconds, data.value)]);
                }
            }

            data_tx.send(Payload {
                query: query.to_string().unwrap(),
                data: facets,
                bounds: Bounds {
                    mins: min_bounds,
                    maxes: max_bounds,
                },
            })?
        }
        sleep(Duration::from_millis(16)).await;
    }
}
