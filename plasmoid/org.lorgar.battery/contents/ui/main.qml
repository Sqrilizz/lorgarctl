import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.components as PlasmaComponents
import org.kde.plasma.plasma5support as Plasma5Support

PlasmoidItem {
    id: root

    property string batteryText: "🖱️ --%"

    Plasmoid.icon: "input-mouse"

    Plasma5Support.DataSource {
        id: executable
        engine: "executable"
        connectedSources: []

        onNewData: function(sourceName, data) {
            if (data["exit code"] === 0 && data.stdout) {
                var out = data.stdout.trim();
                if (out.length > 0) {
                    root.batteryText = out;
                }
            }
            disconnectSource(sourceName);
        }

        function updateBattery() {
            connectSource("/home/sqrilizz/.cargo/bin/lorgarctl battery --short");
        }
    }

    Timer {
        interval: 180000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: {
            executable.updateBattery();
        }
    }

    compactRepresentation: PlasmaComponents.Label {
        text: root.batteryText
        font.pixelSize: 13
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter

        MouseArea {
            anchors.fill: parent
            onClicked: {
                executable.updateBattery();
            }
        }
    }
}
