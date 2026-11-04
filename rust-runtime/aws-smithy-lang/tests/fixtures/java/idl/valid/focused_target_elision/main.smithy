$version: "2"
namespace ex

use other#OtherResource

resource Widget {
    identifiers: { widgetId: WidgetId, region: String }
    properties: { label: String }
    read: GetWidget
}

string WidgetId

@readonly
operation GetWidget {
    input := for Widget {
        @required
        $widgetId

        @required
        $region
    }
    output := for Widget {
        @required
        $widgetId

        @required
        $region

        label: String
    }
}

structure CrossNamespace for OtherResource {
    $otherId
}
