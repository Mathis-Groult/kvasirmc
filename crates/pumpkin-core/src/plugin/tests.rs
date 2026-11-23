use super::*;

mod spawn_golden;

#[tokio::test]
async fn topological_sort() {
    let plugins = vec![
        ("A".to_string(), vec!["B".to_string()]),
        ("B".to_string(), vec!["C".to_string()]),
        ("C".to_string(), vec![]),
    ];
    let sorted = PluginManager::topological_sort(&plugins).unwrap();
    assert_eq!(sorted, vec!["C", "B", "A"]);

    let plugins_complex = vec![
        ("A".to_string(), vec!["B".to_string(), "C".to_string()]),
        ("B".to_string(), vec!["D".to_string()]),
        ("C".to_string(), vec!["D".to_string()]),
        ("D".to_string(), vec![]),
    ];
    let sorted = PluginManager::topological_sort(&plugins_complex).unwrap();
    // Multiple valid sorts possible, but D must be before B and C, and B, C must be before A.
    assert_eq!(sorted[0], "D");
    assert!(sorted[1] == "B" || sorted[1] == "C");
    assert!(sorted[2] == "B" || sorted[2] == "C");
    assert_eq!(sorted[3], "A");

    let plugins_circular = vec![
        ("A".to_string(), vec!["B".to_string()]),
        ("B".to_string(), vec!["A".to_string()]),
    ];
    assert!(PluginManager::topological_sort(&plugins_circular).is_err());

    let plugins_missing = vec![("A".to_string(), vec!["B".to_string()])];
    assert!(PluginManager::topological_sort(&plugins_missing).is_err());
}
