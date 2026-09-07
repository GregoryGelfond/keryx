deployment(r0).
max_per_node(services(r0, 0), 3).
max_per_node(services(r0, 1), 2).
max_per_node(services(r0, 2), 4).
name(services(r0, 0), "web").
name(services(r0, 1), "cache").
name(services(r0, 2), "api").
replicas(services(r0, 0), 0).
replicas(services(r0, 1), 5).
replicas(services(r0, 2), 3).
service(services(r0, 0)).
service(services(r0, 1)).
service(services(r0, 2)).
