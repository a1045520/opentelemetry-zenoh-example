use clap::{App, Arg};
use std::convert::{TryFrom, TryInto};
use zenoh::*;
use futures::prelude::*;
use futures::select;
use async_std::task;
use opentelemetry::trace::TraceError;
use opentelemetry::{
    global,
    sdk::{trace as sdktrace, propagation::TraceContextPropagator},
    trace::{FutureExt, TraceContextExt, Tracer},
    Context,
    KeyValue,
};
use opentelemetry_semantic_conventions::{resource, trace};
use opentelemetry_jaeger;
use std::collections::HashMap;
use std::time;
use serde::{Deserialize, Serialize};
use rand::Rng;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[async_std::main]
async fn main() {
    let (config, collector, action) = parse_args();
    println!("collector path: {} , action: {}", collector, action);

    // initate tracer
    let _ = init_tracer(&action, VERSION,  &collector).unwrap();

    let zenoh = Zenoh::new(config.into()).await.unwrap();
    let workspace = zenoh.workspace(None).await.unwrap();

    if action.as_str() == "sensor" {
        sensor(workspace).await;
    } else if action.as_str() == "computing" {
       computing(workspace).await;
    } else if action.as_str() == "motion"{
        motion(workspace).await;
    }

    zenoh.close().await.unwrap();
    opentelemetry::global::force_flush_tracer_provider();
    opentelemetry::global::shutdown_tracer_provider();
}

#[derive(Serialize, Deserialize, Debug)]
struct Message{
    sleep_time: u64,
    span_context: String,
}

async fn sensor(workspace: zenoh::Workspace<'_>) {
    let tracer = global::tracer("Sensor.rs");
    // Use tracer.start("span_name") could start the span without span builder
    let span = tracer
        .span_builder("generate sensor data")
        .with_attributes(vec![
            trace::MESSAGING_SYSTEM.string("zenoh"),
            trace::MESSAGING_OPERATION.string("send"),
            trace::MESSAGING_DESTINATION.string("computing")
        ])
        .start(&tracer);
    let cx = Context::current_with_span(span);
    let mut injector = HashMap::new();
    // Injector trace context 
    global::get_text_map_propagator(|propagator| propagator.inject_context(&cx, &mut injector));

    // Sleep to simulate taking data from the driver
    let mut rng = rand::thread_rng();
    task::sleep(time::Duration::from_millis(rng.gen_range(0..100))).await;
    
    let message = Message {
        sleep_time: rng.gen_range(50..150),
        span_context: injector["traceparent"].clone(),
    };
    let serialized_message = serde_json::to_string(&message).unwrap();

    cx.span().add_event("sensor data".into(), 
        vec![
            KeyValue::new("sleeping time", message.sleep_time.to_string()),
            KeyValue::new("span context", message.span_context.to_string())
            ]
    );  
    workspace
        .put(&"/sensor_data".try_into().unwrap(), serialized_message.into())
        .with_context(cx.clone())
        .await
        .unwrap();
}

async fn computing(workspace: zenoh::Workspace<'_>) {
    let mut change_stream = workspace
    .subscribe(&"/sensor_data".try_into().unwrap())
    .await
    .unwrap();

    let mut stdin = async_std::io::stdin();
    let mut input = [0u8];
    loop {
        select!(
            change = change_stream.next().fuse() => {
                let change = change.unwrap();
                let mut req_header = HashMap::new();
                if let Value::StringUtf8(value) = change.value.unwrap(){
                    let message: Message = serde_json::from_str(&value).unwrap();
                    req_header.insert("traceparent".to_string(), message.span_context.clone());

                    println!(
                        ">> [Subscription listener] received {:?} for {} : {:?} with timestamp {}",
                        change.kind,
                        change.path,
                        message,
                        change.timestamp
                    );

                    // Extract trace format to get parent context
                    let parent_cx = global::get_text_map_propagator(|propagator| propagator.extract(&req_header));
                    let tracer = global::tracer("Computing.rs");
                    // Use tracer.start_with_context("span_name", parent_cx) could run the span without span builder
                    let span = tracer
                        .span_builder("Get sensor data and start computing")
                        .with_attributes(vec![
                            trace::MESSAGING_SYSTEM.string("zenoh"),
                            trace::MESSAGING_OPERATION.string("process"),
                            trace::MESSAGING_DESTINATION.string("motion")
                        ])
                        .with_parent_context(parent_cx)
                        .start(&tracer);
                    let cx = Context::current_with_span(span);
                    global::get_text_map_propagator(|propagator| propagator.inject_context(&cx, &mut req_header));
                    
                    // Sleep to simulate computing the action
                    task::sleep(time::Duration::from_millis(message.sleep_time)).await;

                    let mut rng = rand::thread_rng();
                    let message = Message {
                        sleep_time: rng.gen_range(0..100),
                        span_context: req_header["traceparent"].clone(),
                    };
                    let serialized_message = serde_json::to_string(&message).unwrap();

                    workspace
                        .put(&"/action".try_into().unwrap(), serialized_message.into())
                        .with_context(cx.clone())
                        .await
                        .unwrap();
                };
            }

            _ = stdin.read_exact(&mut input).fuse() => {
                if input[0] == b'q' {break}
            }
        );
    }
    change_stream.close().await.unwrap();
}

async fn motion(workspace: zenoh::Workspace<'_>) {
    let mut change_stream = workspace
    .subscribe(&"/action".try_into().unwrap())
    .await
    .unwrap();

    let mut stdin = async_std::io::stdin();
    let mut input = [0u8];
    loop {
        select!(
            change = change_stream.next().fuse() => {
                let change = change.unwrap();
                let mut req_header = HashMap::new();
                if let Value::StringUtf8(value) = change.value.unwrap(){
                    let message: Message = serde_json::from_str(&value).unwrap();
                    req_header.insert("traceparent".to_string(), message.span_context.clone());

                    println!(
                        ">> [Subscription listener] received {:?} for {} : {:?} with timestamp {}",
                        change.kind,
                        change.path,
                        message,
                        change.timestamp
                    );
                    
                    let parent_cx = global::get_text_map_propagator(|propagator| propagator.extract(&req_header));
                    let tracer = global::tracer("motion.rs");
                    let _span = tracer
                        .span_builder("Get computing output and start motion")
                        .with_attributes(vec![
                            trace::MESSAGING_SYSTEM.string("zenoh"),
                            trace::MESSAGING_OPERATION.string("receive"),
                        ])
                        .with_parent_context(parent_cx)
                        .start(&tracer);

                    // Sleep to simulate motion control
                    task::sleep(time::Duration::from_millis(message.sleep_time)).await;
                };  
            }

            _ = stdin.read_exact(&mut input).fuse() => {
                if input[0] == b'q' {break}
            }
        );
    }
    change_stream.close().await.unwrap();
}

#[inline]
fn init_tracer(svc_name: &str, version: &str, collector_endpoint: &str) -> Result<sdktrace::Tracer, TraceError> {
    // W3C spec: https://www.w3.org/TR/trace-context/ - only for trace context info
    global::set_text_map_propagator(TraceContextPropagator::new());

    // (Option) A set of standardized attributes, ref: https://github.com/open-telemetry/opentelemetry-specification/tree/main/specification/resource/semantic_conventions
    let tags = [
        resource::SERVICE_VERSION.string(version.to_owned()),
        resource::PROCESS_EXECUTABLE_PATH.string(std::env::current_exe().unwrap().display().to_string()),
        resource::PROCESS_PID.string(std::process::id().to_string()),
    ];

    // Initialize the tracker with jaeger as backend
    opentelemetry_jaeger::new_pipeline()
        .with_service_name(svc_name)
        .with_collector_endpoint(format!("http://{}/api/traces", collector_endpoint))
        .with_tags(tags.iter().map(ToOwned::to_owned))
        .install_batch(opentelemetry::runtime::AsyncStd)
}


fn parse_args() -> (Properties, String, String) {
    let args = App::new("opentelemery-zenoh example")
        .arg(
            Arg::from_usage("-m, --mode=[MODE] 'The zenoh session mode (peer by default).")
                .possible_values(&["peer", "client"]),
        )
        .arg(Arg::from_usage(
            "-e, --peer=[LOCATOR]...  'Peer locators used to initiate the zenoh session.'",
        ))
        .arg(Arg::from_usage(
            "-l, --listener=[LOCATOR]...   'Locators to listen on.'",
        ))
        .arg(Arg::from_usage(
            "-c, --config=[FILE]      'A configuration file.'",
        ))
        .arg(
            Arg::from_usage(
                "-o, --collector=[LOCATOR]      'The address of the collector to collect data'")
                .default_value("localhost:14268"),
        )
        .arg(
            Arg::from_usage("-a, --action=[MODE] 'The action of node (sensor by default).")
                .possible_values(&["sensor", "computing", "motion"]),
        )
        .arg(Arg::from_usage(
            "--no-multicast-scouting 'Disable the multicast-based scouting mechanism.'",
        ))
        .get_matches();

    let mut config = if let Some(conf_file) = args.value_of("config") {
        Properties::try_from(std::path::Path::new(conf_file)).unwrap()
    } else {
        Properties::default()
    };
    for key in ["mode", "peer", "listener"].iter() {
        if let Some(value) = args.values_of(key) {
            config.insert(key.to_string(), value.collect::<Vec<&str>>().join(","));
        }
    }
    if args.is_present("no-multicast-scouting") {
        config.insert("multicast_scouting".to_string(), "false".to_string());
    }

    let collector = args.value_of("collector").unwrap().to_string();
    let action = args.value_of("action").unwrap().to_string();
    
    (config, collector, action)
}
