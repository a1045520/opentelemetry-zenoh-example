# opentelemetry-zenoh-example
Use zenoh to simulate sensors, computing, and motion nodes in different devices to demonstrate distributed tracing with opentelemetry and jaeger.

![Intern drawio](https://user-images.githubusercontent.com/44572922/144536021-75228c32-c01b-41d5-8f72-836d30018233.png)


---

## How to run it
Run the jaeger as backend.
```sh
docker run -d -p6831:6831/udp -p6832:6832/udp -p16686:16686 -p14268:14268 jaegertracing/all-in-one:latest
```
Run the three nodes on different devices.
```sh
# Run the computing and motion node first.
./opentelemetry-zenoh-example -a computing -o <backend_addr>:14268 -e tpc/<motion_ip>:7447
./opentelemetry-zenoh-example -a motion -o <backend_addr>:14268

# Run sensor.
./opentelemetry-zenoh-example -a sensor -o <backend_addr>:14268 -e tpc/<computing_ip>:7447 
```

Observe the results with jaeger.
```sh
# On backend
firefox http://localhost:16686/
```