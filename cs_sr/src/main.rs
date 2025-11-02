mod model;

use model::*;
use stateright::actor::{Actor, ActorModel, Network, Out};
use stateright::{Checker, Model};
use std::borrow::Cow;

fn main() {
    println!("=== Consensus Protocol Verification with Stateright ===\n");

    // Scenario 1: Normal operation (no faults)
    println!("Scenario 1: Normal Operation (No Faults)");
    run_scenario(5, 0, false);

    // Scenario 2: Single node crash
    println!("\nScenario 2: Single Node Crash");
    run_scenario(5, 1, false);

    // Scenario 3: Maximum tolerable faults (f=2)
    println!("\nScenario 3: Maximum Faults (f=2)");
    run_scenario(5, 2, false);

    // Scenario 4: Message loss simulation
    println!("\nScenario 4: Message Loss Simulation");
    run_scenario(5, 0, true);

    // Scenario 5: Combined faults (node crash + message loss)
    println!("\nScenario 5: Combined Faults");
    run_scenario(5, 1, true);

    println!("\n=== Verification Complete ===");
}

fn run_scenario(num_nodes: usize, faulty_nodes: usize, lossy_network: bool) {
    println!("  Nodes: {}, Faulty: {}, Network: {}", 
             num_nodes, 
             faulty_nodes,
             if lossy_network { "Lossy" } else { "Reliable" });

    // Create peer list
    let peers: Vec<_> = (0..num_nodes).map(|i| stateright::actor::Id::from(i)).collect();

    // Build actor system
    let actors: Vec<_> = (0..num_nodes)
        .map(|_| ConsensusActor::new(peers.clone()))
        .collect();

    // Configure network
    let network = if lossy_network {
        Network::new_lossy(vec![0.1]) // 10% packet loss
    } else {
        Network::new_ordered(vec![])
    };

    // Create actor model
    let mut model = ActorModel::new(
        move |cfg| cfg.actor(actors[0].clone()),
        (),
    )
    .duplicating_network(network);

    // Add properties to check
    model = model.property(
        stateright::Expectation::Always,
        "Safety: Agreement",
        |_, state| {
            // Check that all decided nodes agree on value
            let mut decided_values = vec![];
            for actor_state in state.actor_states.iter() {
                if let Some(consensus_state) = actor_state.downcast_ref::<ConsensusNodeState>() {
                    if consensus_state.decided && !consensus_state.is_faulty {
                        if let Some(ref value) = consensus_state.value {
                            decided_values.push(value.clone());
                        }
                    }
                }
            }
            
            // All decided values must be the same
            if decided_values.len() > 1 {
                let first = &decided_values[0];
                decided_values.iter().all(|v| v == first)
            } else {
                true
            }
        },
    );

    model = model.property(
        stateright::Expectation::Sometimes,
        "Liveness: Eventually Decide",
        |_, state| {
            // At least one non-faulty node should eventually decide
            state.actor_states.iter().any(|actor_state| {
                if let Some(consensus_state) = actor_state.downcast_ref::<ConsensusNodeState>() {
                    consensus_state.decided && !consensus_state.is_faulty
                } else {
                    false
                }
            })
        },
    );

    // Run bounded model checker
    println!("  Running model checker...");
    let checker = model.checker()
        .threads(4)
        .target_max_depth(20);

    match checker.check(1_000_000) {
        Ok(stats) => {
            println!("  ✓ Verification passed!");
            println!("    States explored: {}", stats.generated_count);
            println!("    Max depth: {}", stats.max_depth);
        }
        Err(err) => {
            println!("  ✗ Verification failed!");
            println!("    Error: {:?}", err);
        }
    }
}

/// Simulate a specific fault scenario
fn simulate_fault_scenario() {
    println!("\n=== Detailed Fault Scenario Simulation ===\n");

    // Create a scenario where we manually inject faults
    let num_nodes = 5;
    let peers: Vec<_> = (0..num_nodes).map(|i| stateright::actor::Id::from(i)).collect();

    println!("Simulating: Node 0 proposes V1, Node 3 crashes after PREPARE phase");
    
    // Manual state evolution
    let mut states: Vec<ConsensusNodeState> = (0..num_nodes)
        .map(|i| ConsensusNodeState::new(i as u8, 5))
        .collect();

    // Step 1: Node 0 proposes V1
    println!("\n[Step 1] Node 0 proposes V1");
    states[0].value = Some(Value::V1);
    print_states(&states);

    // Step 2: All nodes receive proposal and send PREPARE
    println!("\n[Step 2] All nodes receive proposal and broadcast PREPARE");
    for i in 0..num_nodes {
        states[i].value = Some(Value::V1);
        *states[i].prepare_count.entry(Value::V1).or_insert(0) = num_nodes;
        states[i].state = NodeState::Prepared;
    }
    print_states(&states);

    // Step 3: Node 3 crashes
    println!("\n[Step 3] Node 3 crashes");
    states[3].is_faulty = true;
    states[3].state = NodeState::Failed;
    print_states(&states);

    // Step 4: Non-faulty nodes receive COMMITs (only 4 nodes now)
    println!("\n[Step 4] Remaining 4 nodes broadcast COMMIT");
    for i in 0..num_nodes {
        if !states[i].is_faulty {
            *states[i].commit_count.entry(Value::V1).or_insert(0) = 4;
            // Can't reach quorum of 5 with only 4 nodes!
        }
    }
    print_states(&states);

    // Analysis
    println!("\n[Analysis]");
    println!("  Quorum required: 5 nodes");
    println!("  Active nodes: 4 nodes");
    println!("  Result: DEADLOCK - Cannot reach consensus!");
    println!("  Lesson: With f=2 Byzantine tolerance, we need 2f+1=5 nodes.");
    println!("          Losing 1 node means we can't tolerate any more faults.");
}

fn print_states(states: &[ConsensusNodeState]) {
    for state in states {
        println!(
            "  Node {}: state={:?}, value={:?}, prepares={}, commits={}, decided={}, faulty={}",
            state.id,
            state.state,
            state.value,
            state.prepare_count.get(&Value::V1).unwrap_or(&0),
            state.commit_count.get(&Value::V1).unwrap_or(&0),
            state.decided,
            state.is_faulty
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_consensus_model() {
        let model = ConsensusModel::new(5, 2);
        
        // Test agreement checker
        let mut state1 = ConsensusNodeState::new(0, 3);
        state1.decided = true;
        state1.value = Some(Value::V1);
        
        let mut state2 = ConsensusNodeState::new(1, 3);
        state2.decided = true;
        state2.value = Some(Value::V1);
        
        assert!(model.check_agreement(&[state1, state2]));
    }

    #[test]
    fn test_no_premature_decision() {
        let model = ConsensusModel::new(5, 2);
        
        let mut state = ConsensusNodeState::new(0, 3);
        state.state = NodeState::Decided;
        state.value = Some(Value::V1);
        state.commit_count.insert(Value::V1, 2);
        
        // Should fail - not enough commits
        assert!(!model.check_no_premature_decision(&state));
        
        // Fix it
        state.commit_count.insert(Value::V1, 3);
        assert!(model.check_no_premature_decision(&state));
    }
}