#![feature(proc_macro_hygiene, decl_macro)]

/// simple redis-using worker service

use std;
use rand;
use tracing;
use tracing_subscriber;
extern crate bcrypt;
#[macro_use] extern crate rocket;
use rocket_contrib;

/// entrypoint

fn main() {
    // logging (with RUST_LOG)
    tracing_subscriber::fmt::init();
    // arg validation
    let args: std::vec::Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        tracing::error!("Exactly one of the 'server' or 'worker' arguments is required.");
        return;
    }
    // pubsub config
    let topic = std::env::var("PUBSUB_TOPIC")
        .expect("The 'PUBSUB_TOPIC' environment variable must contain the topic name, got");
    let addr = std::env::var("REDIS_URI")
        .expect("The 'REDIS_URI' environment variable must contain the redis connection string, got");
    let client = redis::Client::open(addr.clone())
        .expect("Unable to create Redis client");
    let pool = WorkerPool { addr, client, topic };
    // run the service
    if args[1] == "server" {
        server(pool);
    } else if args[1] == "worker" {
        worker(pool);
    }
}

/// data

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct Job {
    reply_to: String,
    content: String,
}

struct  WorkerPool {
    addr: String,
    client: redis::Client,
    topic: String
}

impl std::fmt::Debug for WorkerPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerPool")
         .field("addr", &self.addr)
         .field("topic", &self.topic)
         .finish()
    }
}

/// worker

#[tracing::instrument(skip(conn))]
fn handle_job(conn: &mut redis::Connection, job: Job) {
    use redis::Commands;

    // lock the job, or do nothing if not locked
    // SET <job.reply_to> "locked" NX EX 30
    let locked: redis::Value = redis::cmd("SET")
        .arg(job.reply_to.clone())
        .arg("locked")
        .arg("NX")
        .arg("EX")
        .arg("30")
        .query(conn)
        .expect("Unable to lock job");
    if locked != redis::Value::Okay {
        tracing::debug!(
            input = job.content.as_str(),
            reply_to = job.reply_to.as_str(),
            locked = true
        );
        return;
    } else {
        tracing::debug!(
            input = job.content.as_str(),
            reply_to = job.reply_to.as_str(),
            locked = false
        );
    }
    
    // bcrypt to simulate important work
    let work_factor: u32 = std::env::var("WORK_FACTOR").unwrap_or("14".to_string()).parse()
        .expect("Unable to parse 'WORK_FACTOR' as an integer");
    let msg = bcrypt::hash(job.content.clone(), work_factor).expect("Unable to apply bcrypt to input");
    tracing::info!(
        work_factor = work_factor,
        input = job.content.as_str(),
        output = msg.as_str(),
        reply_to = job.reply_to.as_str()
    );
    let _: usize = conn.publish(job.reply_to, msg).expect("Unable to publish output");
}

#[tracing::instrument]
fn worker(pool: WorkerPool) {
    tracing::info!("starting worker");
    // connect to redis
    let mut listener = pool.client.get_connection()
        .expect("Unable to connect to Redis");
    let mut replier = pool.client.get_connection()
        .expect("Unable to connect to Redis");
    // subscribe to the redis topic
    let mut subscriber = listener.as_pubsub();
    subscriber.subscribe(pool.topic).expect("Unable to subscribe to job topic");
    loop {
        // each payload is a job; deserialize and handle it
        let msg = subscriber.get_message().expect("Unable to recieve message");
        let payload : String = msg.get_payload().expect("Unable to retrieve message payload");
        let job: Job = serde_json::from_str(&payload).expect("Unable to deserialize job");
        handle_job(&mut replier, job);
    }
}

/// server

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct TransformRequest {
    content: String,
}
#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct TransformResponse {
    content: String
}

#[post("/", format = "json", data = "<body>")]
fn transform(
    body: rocket_contrib::json::Json<TransformRequest>, 
    pool: rocket::State<WorkerPool>
) -> rocket_contrib::json::Json<TransformResponse> {
    use redis::Commands;
    use rand::Rng;
    // connect to redis
    let mut conn = pool.client.get_connection()
        .expect("Unable to connect to Redis");
    // publish the job
    let reply_to: String = format!("{}-{}", pool.topic.clone(), rand::thread_rng().gen::<usize>());
    let job = Job { 
        reply_to: reply_to.clone(), 
        content: body.content.clone() 
    };
    let msg = serde_json::to_string(&job).expect("Unable to serialize job");
    let _: usize = conn.publish(pool.topic.clone(), msg).expect("Unable to publish job");
    // subscribe with a 10-second timeout
    let mut subscriber = conn.as_pubsub();
    subscriber.set_read_timeout(Some(core::time::Duration::new(10, 0))).expect("Unable to set subscribe timeout");
    tracing::debug!(
        input = body.content.as_str(),
        reply_to = reply_to.as_str(),
        "waiting"
    );
    subscriber.subscribe(reply_to).expect("Unable to subscribe to response topic");
    // response topic just contains the message
    let msg = subscriber.get_message().expect("Unable to recieve message");
    let payload : String = msg.get_payload().expect("Unable to retrieve message payload");
    rocket_contrib::json::Json(TransformResponse { content: payload })
}

#[tracing::instrument]
fn server(pool: WorkerPool) {
    tracing::info!("starting server");
    rocket::ignite()
        .manage(pool)
        .mount("/", routes![transform])
        .launch();
}
