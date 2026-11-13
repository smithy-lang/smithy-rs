$version: "2"

namespace ex

use other#Imported

// Targets resolve like other relative shape IDs: through `use`, or written as absolute IDs.
apply Imported @documentation("Through use.")

apply other#Absolute @documentation("Absolute.")
