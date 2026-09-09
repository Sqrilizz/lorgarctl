import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.components as PlasmaComponents
import org.kde.plasma.plasma5support as Plasma5Support

PlasmoidItem {
    id: root

    property string batteryText: "Unknown"

    Plasmoid.icon: "input-mouse"
    Plasmoid.title: "LORGAR Battery"

    Plasma5Support.DataSource {
        id: executable
        engine: "executable"
        connectedSources: []

        onNewData: function(sourceName, data) {
            if (data["exit code"] === 0 && data.stdout) {
                var out = data.stdout.trim();
                if (out.length > 0) {
                    root.batteryText = out.replace("🖱️", "").trim();
                }
            } else {
                root.batteryText = "Unavailable";
            }
            disconnectSource(sourceName);
        }

        function updateBattery() {
            connectSource("lorgarctl --daemon battery --short");
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

    compactRepresentation: RowLayout {
        spacing: 4

        PlasmaComponents.Label {
            text: "🖱️ " + root.batteryText
            font.pixelSize: 13
            Layout.alignment: Qt.AlignVCenter
        }

        MouseArea {
            anchors.fill: parent
            onClicked: {
                executable.updateBattery();
            }
        }
    }
}
