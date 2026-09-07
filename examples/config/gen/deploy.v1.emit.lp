#include "deploy.v1.core.lp".
%!totality of code : finding -> int32  (total)
has_code(P) :- code(P, _).
%!totality of max_per_node : service -> int32  (total)
has_max_per_node(P) :- max_per_node(P, _).
%!totality of name : service -> string  (total)
has_name(P) :- name(P, _).
%!totality of reason : finding -> string  (total)
has_reason(P) :- reason(P, _).
%!totality of replicas : service -> int32  (total)
has_replicas(P) :- replicas(P, _).
%!contiguity of services : deployment × index -> service  (sequence)
has_services(P, I) :- service(services(P, I)).
%!services : deployment × index -> service  (sequence)
reach(A) :- deployment(X), reach(X), service(A), A = services(X, I).
%!findings : report × string -> finding  (map)
reach(A) :- finding(A), reach(X), report(X), A = findings(X, K).
%!emit_deployment : deployment  (root)
reach(X) :- emit_deployment(X).
%!emit_finding : finding  (root)
reach(X) :- emit_finding(X).
%!emit_report : report  (root)
reach(X) :- emit_report(X).
%!emit_service : service  (root)
reach(X) :- emit_service(X).
%!functionality of code : finding -> int32  (total)
:- code(P, V1), code(P, V2), finding(P), reach(P), V1 != V2.
%!occupancy of emit_finding : finding  (root)
:- code(P, _), emit_finding(P), not finding(P).
%!occupancy of findings : report × string -> finding  (map)
:- code(findings(X, K), _), reach(X), report(X), not finding(findings(X, K)).
%!contiguity of services : deployment × index -> service  (sequence)
:- deployment(P), has_services(P, I), reach(P), I > 0, not has_services(P, (I - 1)).
%!contiguity of services : deployment × index -> service  (sequence)
:- deployment(P), reach(P), service(services(P, I)), I < 0.
%!occupancy of services : deployment × index -> service  (sequence)
:- deployment(X), max_per_node(services(X, I), _), reach(X), not service(services(X, I)).
%!occupancy of services : deployment × index -> service  (sequence)
:- deployment(X), name(services(X, I), _), reach(X), not service(services(X, I)).
%!occupancy of services : deployment × index -> service  (sequence)
:- deployment(X), reach(X), replicas(services(X, I), _), not service(services(X, I)).
%!occupancy of emit_deployment : deployment  (root)
:- emit_deployment(P), service(services(P, _)), not deployment(P).
%!occupancy of emit_finding : finding  (root)
:- emit_finding(P), reason(P, _), not finding(P).
%!occupancy of emit_report : report  (root)
:- emit_report(P), finding(findings(P, _)), not report(P).
%!occupancy of emit_service : service  (root)
:- emit_service(P), max_per_node(P, _), not service(P).
%!occupancy of emit_service : service  (root)
:- emit_service(P), name(P, _), not service(P).
%!occupancy of emit_service : service  (root)
:- emit_service(P), replicas(P, _), not service(P).
%!functionality of reason : finding -> string  (total)
:- finding(P), reach(P), reason(P, V1), reason(P, V2), V1 != V2.
%!totality of code : finding -> int32  (total)
:- finding(P), reach(P), not has_code(P).
%!totality of reason : finding -> string  (total)
:- finding(P), reach(P), not has_reason(P).
%!functionality of max_per_node : service -> int32  (total)
:- max_per_node(P, V1), max_per_node(P, V2), reach(P), service(P), V1 != V2.
%!functionality of name : service -> string  (total)
:- name(P, V1), name(P, V2), reach(P), service(P), V1 != V2.
%!functionality of replicas : service -> int32  (total)
:- reach(P), replicas(P, V1), replicas(P, V2), service(P), V1 != V2.
%!totality of max_per_node : service -> int32  (total)
:- reach(P), service(P), not has_max_per_node(P).
%!totality of name : service -> string  (total)
:- reach(P), service(P), not has_name(P).
%!totality of replicas : service -> int32  (total)
:- reach(P), service(P), not has_replicas(P).
%!occupancy of findings : report × string -> finding  (map)
:- reach(X), reason(findings(X, K), _), report(X), not finding(findings(X, K)).
%!emit_deployment : deployment  (root)
#defined emit_deployment/1.
%!emit_finding : finding  (root)
#defined emit_finding/1.
%!emit_report : report  (root)
#defined emit_report/1.
%!emit_service : service  (root)
#defined emit_service/1.
