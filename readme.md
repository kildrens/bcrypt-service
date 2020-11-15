# bcrypt service

This service provides a mechanism for outsourcing expensive bcrypt computation to a pool of workers.

It exposes a single HTTP endpoint `\`, which takes a JSON structure containing one field, `content`, which is the string to apply bcrypt to. It will return an equivalent JSON structure, with `content` containing the result after bcrypt is applied.

An example with `curl`:
```
$ curl --header "Content-Type: application/json" \
  --request POST \
  --data '{"content": "hunter2"}' \
  http://localhost:8000/
{"content":"$2b$14$HBhe4KmS52vfahz91YZ0puShGtJs9R6n2zQHASCpHGzEj8qwj5NKm"}
```

Binary releases are provided via Github for Ubuntu.

## implementation

The service has three components:
  - the HTTP API server which provides the consumer interface
  - a worker pool to distribute work across compute resources
  - Redis for communication between the server and workers

When the server receives a request, it publishes a job on a Redis PubSub topic (the job topic), and starts subscribing to a new per-request topic. The input content and per-request topic are included in the job. Each worker in the pool listens to the job topic, and competes for incoming jobs (locking the job to ensure no other worker takes it). Once the worker has computed the bcrypt result, it publishes that result on the per-request topic. The server, which was listening on that topic, then returns that value to the client.

The server and the worker are implemented in the same binary. Runing `bcrypt-service worker` will run the worker, and running `bcrypt-service server` will run the server. 

Configuration of the worker & server is via environmental variables:


|Env Variable|Default Value|Description|
|---|---|---|
|REDIS_URI|n/a|Redis connection string. e.g. `redis://localhost/0`.|
|PUBSUB_TOPIC|n/a|Pubsub job topic name. Must match between server and workers.|
|ROCKET_PORT|8000|Port to listen for HTTP requests on. Server only.|
|ROCKET_ADDRESS|0.0.0.0|Address to listen for HTTP requests on. Server only.|
|WORK_FACTOR|14|bcrypt work factor. Worker only.|
|RUST_LOG|`warning`|Log level, one of `trace`, `debug`, `info`, `warning`, `error`.|


The binary is implemented in nightly [Rust](https://rustup.rs/), and building from source can be done with with `cargo build`.