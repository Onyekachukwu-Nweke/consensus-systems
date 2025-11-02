---------------------------- MODULE ConsensusSystem ----------------------------
(*
  Simplified PBFT-like Consensus Protocol for Crypto ETF Platform
  
  This specification models a distributed consensus system where nodes must
  agree on an on-chain transaction value (e.g., ETF asset allocation decision).
  
  Key Features:
  - Byzantine Fault Tolerance (simplified)
  - Three-phase protocol: PREPARE -> COMMIT -> DECIDE
  - Quorum-based agreement (2f+1 nodes for tolerating f faults)
  - Asynchronous message delivery with potential loss
*)

EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS 
    Nodes,          \* Set of all nodes in the system
    Values,         \* Set of possible values to agree on
    MaxFaults,      \* Maximum number of faulty nodes (f)
    MaxMsgLoss      \* Maximum messages that can be lost

VARIABLES
    nodeState,      \* nodeState[n] = current state of node n
    nodeValue,      \* nodeValue[n] = value node n proposes/accepts
    prepareCount,   \* prepareCount[n][v] = # of PREPARE messages for value v at node n
    commitCount,    \* commitCount[n][v] = # of COMMIT messages for value v at node n
    decided,        \* decided[n] = TRUE if node n has decided
    messages,       \* Set of messages in transit
    lostMessages,   \* Counter for lost messages
    phase          \* Global phase counter for liveness checking

vars == <<nodeState, nodeValue, prepareCount, commitCount, decided, messages, lostMessages, phase>>

\* Node states
States == {"INIT", "PREPARED", "COMMITTED", "DECIDED", "FAILED"}

\* Message types
Message == [type: {"PROPOSE", "PREPARE", "COMMIT", "DECIDE"}, 
            src: Nodes, 
            dst: Nodes, 
            value: Values]

\* Quorum size: 2f+1 nodes needed for consensus
Quorum == (2 * MaxFaults) + 1

-----------------------------------------------------------------------------
\* Type Invariants

TypeOK == 
    /\ nodeState \in [Nodes -> States]
    /\ nodeValue \in [Nodes -> Values \cup {0}]  \* 0 = no value yet
    /\ prepareCount \in [Nodes -> [Values -> 0..Cardinality(Nodes)]]
    /\ commitCount \in [Nodes -> [Values -> 0..Cardinality(Nodes)]]
    /\ decided \in [Nodes -> BOOLEAN]
    /\ messages \subseteq Message
    /\ lostMessages \in 0..MaxMsgLoss
    /\ phase \in Nat

-----------------------------------------------------------------------------
\* Initial State

Init ==
    /\ nodeState = [n \in Nodes |-> "INIT"]
    /\ nodeValue = [n \in Nodes |-> 0]
    /\ prepareCount = [n \in Nodes |-> [v \in Values |-> 0]]
    /\ commitCount = [n \in Nodes |-> [v \in Values |-> 0]]
    /\ decided = [n \in Nodes |-> FALSE]
    /\ messages = {}
    /\ lostMessages = 0
    /\ phase = 0

-----------------------------------------------------------------------------
\* Helper Functions

IsNonFaulty(n) == nodeState[n] /= "FAILED"

ActiveNodes == {n \in Nodes : IsNonFaulty(n)}

HasQuorum(count) == count >= Quorum

SendMessage(m) == messages' = messages \cup {m}

BroadcastMessage(src, msgType, val) ==
    messages' = messages \cup {[type |-> msgType, 
                                 src |-> src, 
                                 dst |-> dst, 
                                 value |-> val] : dst \in Nodes}

-----------------------------------------------------------------------------
\* Actions

\* A node proposes a value (typically the primary/leader)
Propose(n, v) ==
    /\ nodeState[n] = "INIT"
    /\ IsNonFaulty(n)
    /\ nodeValue[n] = 0
    /\ nodeValue' = [nodeValue EXCEPT ![n] = v]
    /\ BroadcastMessage(n, "PROPOSE", v)
    /\ nodeState' = [nodeState EXCEPT ![n] = "INIT"]  \* Still in INIT until receives prepares
    /\ UNCHANGED <<prepareCount, commitCount, decided, lostMessages, phase>>

\* A node receives a PROPOSE message and sends PREPARE
ReceivePropose(n) ==
    /\ \E m \in messages :
        /\ m.type = "PROPOSE"
        /\ m.dst = n
        /\ IsNonFaulty(n)
        /\ nodeState[n] = "INIT"
        /\ nodeValue[n] = 0  \* Haven't accepted a value yet
        /\ nodeValue' = [nodeValue EXCEPT ![n] = m.value]
        /\ BroadcastMessage(n, "PREPARE", m.value)
        /\ messages' = messages \ {m}  \* Remove processed message
        /\ UNCHANGED <<nodeState, prepareCount, commitCount, decided, lostMessages, phase>>

\* A node receives PREPARE messages
ReceivePrepare(n) ==
    /\ \E m \in messages :
        /\ m.type = "PREPARE"
        /\ m.dst = n
        /\ IsNonFaulty(n)
        /\ m.value = nodeValue[n]  \* Only count prepares for our value
        /\ prepareCount' = [prepareCount EXCEPT 
                             ![n] = [@ EXCEPT ![m.value] = @ + 1]]
        /\ messages' = messages \ {m}
        /\ IF HasQuorum(prepareCount[n][m.value] + 1) /\ nodeState[n] = "INIT"
           THEN /\ nodeState' = [nodeState EXCEPT ![n] = "PREPARED"]
                /\ BroadcastMessage(n, "COMMIT", m.value)
           ELSE /\ nodeState' = nodeState
                /\ messages' = messages \ {m}
        /\ UNCHANGED <<nodeValue, commitCount, decided, lostMessages, phase>>

\* A node receives COMMIT messages
ReceiveCommit(n) ==
    /\ \E m \in messages :
        /\ m.type = "COMMIT"
        /\ m.dst = n
        /\ IsNonFaulty(n)
        /\ nodeState[n] = "PREPARED"
        /\ m.value = nodeValue[n]
        /\ commitCount' = [commitCount EXCEPT 
                            ![n] = [@ EXCEPT ![m.value] = @ + 1]]
        /\ messages' = messages \ {m}
        /\ IF HasQuorum(commitCount[n][m.value] + 1)
           THEN /\ nodeState' = [nodeState EXCEPT ![n] = "COMMITTED"]
                /\ BroadcastMessage(n, "DECIDE", m.value)
           ELSE /\ nodeState' = nodeState
                /\ messages' = messages \ {m}
        /\ UNCHANGED <<nodeValue, prepareCount, decided, lostMessages, phase>>

\* A node receives DECIDE message and finalizes
ReceiveDecide(n) ==
    /\ \E m \in messages :
        /\ m.type = "DECIDE"
        /\ m.dst = n
        /\ IsNonFaulty(n)
        /\ m.value = nodeValue[n]
        /\ decided' = [decided EXCEPT ![n] = TRUE]
        /\ nodeState' = [nodeState EXCEPT ![n] = "DECIDED"]
        /\ messages' = messages \ {m}
        /\ UNCHANGED <<nodeValue, prepareCount, commitCount, lostMessages, phase>>

\* Message loss (simulating network unreliability)
LoseMessage ==
    /\ lostMessages < MaxMsgLoss
    /\ messages /= {}
    /\ \E m \in messages :
        /\ messages' = messages \ {m}
        /\ lostMessages' = lostMessages + 1
    /\ UNCHANGED <<nodeState, nodeValue, prepareCount, commitCount, decided, phase>>

\* Node crash (Byzantine fault)
NodeCrash ==
    /\ \E n \in Nodes :
        /\ IsNonFaulty(n)
        /\ Cardinality({node \in Nodes : nodeState[node] = "FAILED"}) < MaxFaults
        /\ nodeState' = [nodeState EXCEPT ![n] = "FAILED"]
    /\ UNCHANGED <<nodeValue, prepareCount, commitCount, decided, messages, lostMessages, phase>>

\* Phase progression (for liveness checking)
PhaseAdvance ==
    /\ phase' = phase + 1
    /\ UNCHANGED <<nodeState, nodeValue, prepareCount, commitCount, decided, messages, lostMessages>>

-----------------------------------------------------------------------------
\* Next State Relation

Next ==
    \/ \E n \in Nodes, v \in Values : Propose(n, v)
    \/ \E n \in Nodes : ReceivePropose(n)
    \/ \E n \in Nodes : ReceivePrepare(n)
    \/ \E n \in Nodes : ReceiveCommit(n)
    \/ \E n \in Nodes : ReceiveDecide(n)
    \/ LoseMessage
    \/ NodeCrash
    \/ PhaseAdvance

-----------------------------------------------------------------------------
\* Safety Properties

\* Agreement: No two non-faulty nodes decide on different values
Agreement == 
    \A n1, n2 \in Nodes :
        (decided[n1] /\ decided[n2] /\ IsNonFaulty(n1) /\ IsNonFaulty(n2))
        => (nodeValue[n1] = nodeValue[n2])

\* Validity: If a value is decided, it was proposed by some node
Validity ==
    \A n \in Nodes :
        (decided[n] /\ IsNonFaulty(n)) => nodeValue[n] \in Values

\* Integrity: Each node decides at most once
Integrity ==
    \A n \in Nodes :
        decided[n] => nodeState[n] = "DECIDED"

\* No Premature Decision: Can't decide without quorum of commits
NoPrematureDecision ==
    \A n \in Nodes :
        (nodeState[n] = "DECIDED") => 
            commitCount[n][nodeValue[n]] >= Quorum

-----------------------------------------------------------------------------
\* Liveness Properties (as temporal formulas)

\* Eventual Decision: If enough nodes are non-faulty, eventually they decide
\* Note: This is only guaranteed if messages eventually arrive
EventualDecision ==
    (Cardinality(ActiveNodes) >= Quorum) ~> 
    (\E n \in Nodes : decided[n] /\ IsNonFaulty(n))

\* Progress: The system keeps making progress
Progress ==
    (phase < 100) \/ (\E n \in Nodes : decided[n])

\* State constraint to limit exploration
StateConstraint == phase < 10

-----------------------------------------------------------------------------
\* Specification

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

-----------------------------------------------------------------------------
\* Invariants to Check

SafetyInvariant == Agreement /\ Validity /\ Integrity /\ NoPrematureDecision

=============================================================================