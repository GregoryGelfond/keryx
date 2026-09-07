% The consuming tool's model — keryx neither ships nor runs it. keryx wires the two ends
% (`keryx facts` in, `keryx emit` out); this is the solve in between, run by *your* clingo over
% the vocabulary `keryx gen` produced from deployment.proto. It has no choice rules, so it is
% deterministic: one Deployment in, one Report out.

% --- Invariants over the shredded Deployment facts ---
bad_replicas(S) :- replicas(S, R), R < 1.
bad_capacity(S) :- replicas(S, R), max_per_node(S, M), R > M.

% One finding per service — replicas-below-one is reported ahead of over-capacity.
finding_for(S, "replicas must be at least 1", 1) :- bad_replicas(S).
finding_for(S, "replicas exceeds max_per_node", 2) :- bad_capacity(S), not bad_replicas(S).

% --- Build the Report keryx will emit, keyed by service name ---
report(v0).
finding(findings(v0, N)) :- finding_for(S, _, _), name(S, N).
reason(findings(v0, N), R) :- finding_for(S, R, _), name(S, N).
code(findings(v0, N), C) :- finding_for(S, _, C), name(S, N).
emit_report(v0).
