# Implementation plan and status

Phases A-D in the README are complete. The RFD remains active for one shared
integration gate:

- scoped, borrow-capable tasks with real wakers and join/cancel/drain are
  tracked as Trait Solving Step 2;
- the quiescence/fallback-finalization body-completion state machine is tracked
  as Trait Solving Step 11;
- Method Resolution consumes both through retained lookup obligations.

Those checkboxes live in the Trait Solving implementation plan so one landed
change has one authoritative per-step status. When that gate lands, complete
this RFD's lifecycle update together with the roadmap and architecture pages.
