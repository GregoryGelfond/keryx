% The consuming tool's clingo, run over facts.bad.lp + model.lp, derives this Report — the
% validation findings for config.bad. `keryx emit --root Report answer.bad.lp deployment.proto -I .`
% reassembles it to report.bad.binpb. A real clingo answer set also carries the working atoms
% (reach/1, has_*, bad_replicas/1, finding_for/3, and the input Deployment facts); the reassembler
% keys only on the sort, field, and marker atoms, so they do not change the result.
report(v0).
emit_report(v0).
finding(findings(v0, "web")).
reason(findings(v0, "web"), "replicas must be at least 1").
code(findings(v0, "web"), 1).
finding(findings(v0, "cache")).
reason(findings(v0, "cache"), "replicas exceeds max_per_node").
code(findings(v0, "cache"), 2).
