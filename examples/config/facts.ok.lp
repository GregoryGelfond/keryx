deployment(r0).
max_per_node(services(r0, 0), 3).
max_per_node(services(r0, 1), 4).
name(services(r0, 0), "web").
name(services(r0, 1), "api").
replicas(services(r0, 0), 2).
replicas(services(r0, 1), 3).
service(services(r0, 0)).
service(services(r0, 1)).
