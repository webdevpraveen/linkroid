// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick
import QtQuick.Shapes

// The uninstaller's mascot: drooping droid antennas, a frown, and now and
// then a small tear. Drawn on a 140 × 120 grid and scaled to fit.
Item {
    id: mascot

    property bool animate: true
    property bool dark: false
    property color bodyColor: dark ? "#242E3D" : "#E2E8F0"
    property color outlineColor: dark ? "#3B4D63" : "#CBD5E1"
    property color headColor: dark ? "#192231" : "#F1F5F9"
    property color accentColor: dark ? "#38BDF8" : "#0284C7"
    property color eyeColor: dark ? "#E2E8F0" : "#1E293B"
    property color earColor: dark ? "#2E3B4E" : "#94A3B8"
    property color tearColor: dark ? "#38BDF8" : "#0284C7"

    implicitWidth: 140
    implicitHeight: 120

    Accessible.role: Accessible.Graphic
    Accessible.name: qsTr("A sad mascot")

    Item {
        id: art
        width: 140
        height: 120
        anchors.centerIn: parent
        scale: Math.min(mascot.width / 140, mascot.height / 120)

        // A slow, heavy bob.
        property real bob: 0
        SequentialAnimation on bob {
            running: mascot.animate
            loops: Animation.Infinite
            NumberAnimation { from: 0; to: 3; duration: 1900; easing.type: Easing.InOutSine }
            NumberAnimation { from: 3; to: 0; duration: 1900; easing.type: Easing.InOutSine }
        }
        transform: Translate { y: art.bob }

        // Droid antennae (drooping slightly).
        Shape {
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer

            // Left antenna stalk
            ShapePath {
                strokeWidth: 2.4
                strokeColor: mascot.earColor
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                startX: 55; startY: 34
                PathQuad { x: 44; y: 20; controlX: 47; controlY: 26 }
            }
            // Left antenna tip
            ShapePath {
                strokeWidth: 0
                strokeColor: "transparent"
                fillColor: mascot.accentColor
                PathAngleArc { centerX: 44; centerY: 20; radiusX: 3.5; radiusY: 3.5; startAngle: 0; sweepAngle: 360 }
            }

            // Right antenna stalk
            ShapePath {
                strokeWidth: 2.4
                strokeColor: mascot.earColor
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                startX: 85; startY: 34
                PathQuad { x: 96; y: 20; controlX: 93; controlY: 26 }
            }
            // Right antenna tip
            ShapePath {
                strokeWidth: 0
                strokeColor: "transparent"
                fillColor: mascot.accentColor
                PathAngleArc { centerX: 96; centerY: 20; radiusX: 3.5; radiusY: 3.5; startAngle: 0; sweepAngle: 360 }
            }
        }

        // Side ear pods
        Rectangle {
            x: 29; y: 52
            width: 5; height: 16
            radius: 2.5
            color: mascot.earColor
        }
        Rectangle {
            x: 106; y: 52
            width: 5; height: 16
            radius: 2.5
            color: mascot.earColor
        }

        // Droid Head / Dome
        Shape {
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer

            // Main head base
            ShapePath {
                strokeWidth: 1.5
                strokeColor: mascot.outlineColor
                fillColor: mascot.headColor
                startX: 34; startY: 60
                PathAngleArc { centerX: 70; centerY: 60; radiusX: 36; radiusY: 30; startAngle: 0; sweepAngle: 360 }
            }

            // Visor screen / eye region
            ShapePath {
                strokeWidth: 0
                strokeColor: "transparent"
                fillColor: mascot.bodyColor
                PathAngleArc { centerX: 70; centerY: 58; radiusX: 28; radiusY: 18; startAngle: 0; sweepAngle: 360 }
            }

            // Left eye (sad look)
            ShapePath {
                strokeWidth: 0
                strokeColor: "transparent"
                fillColor: mascot.eyeColor
                PathAngleArc { centerX: 58; centerY: 58; radiusX: 4; radiusY: 4; startAngle: 0; sweepAngle: 360 }
            }
            ShapePath {
                strokeWidth: 0
                strokeColor: "transparent"
                fillColor: "#FFFFFF"
                PathAngleArc { centerX: 57; centerY: 57; radiusX: 1.4; radiusY: 1.4; startAngle: 0; sweepAngle: 360 }
            }

            // Right eye (sad look)
            ShapePath {
                strokeWidth: 0
                strokeColor: "transparent"
                fillColor: mascot.eyeColor
                PathAngleArc { centerX: 82; centerY: 58; radiusX: 4; radiusY: 4; startAngle: 0; sweepAngle: 360 }
            }
            ShapePath {
                strokeWidth: 0
                strokeColor: "transparent"
                fillColor: "#FFFFFF"
                PathAngleArc { centerX: 81; centerY: 57; radiusX: 1.4; radiusY: 1.4; startAngle: 0; sweepAngle: 360 }
            }

            // Eyebrows slanted up in the middle (sad expression)
            ShapePath {
                strokeWidth: 1.6
                strokeColor: mascot.eyeColor
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                startX: 52; startY: 51
                PathLine { x: 62; y: 49 }
            }
            ShapePath {
                strokeWidth: 1.6
                strokeColor: mascot.eyeColor
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                startX: 88; startY: 51
                PathLine { x: 78; y: 49 }
            }

            // Small frown
            ShapePath {
                strokeWidth: 1.8
                strokeColor: mascot.eyeColor
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                startX: 65; startY: 69
                PathQuad { x: 75; y: 69; controlX: 70; controlY: 66 }
            }
        }

        // Now and then, a tear.
        Rectangle {
            id: tear
            x: 54
            width: 4
            height: 5.5
            radius: 2
            color: mascot.tearColor
            visible: mascot.animate
            property real fall: 0
            y: 62 + fall * 14
            opacity: fall < 0.05 ? fall * 20 : 1 - fall
            SequentialAnimation on fall {
                running: mascot.animate
                loops: Animation.Infinite
                PauseAnimation { duration: 3200 }
                NumberAnimation { from: 0; to: 1; duration: 1500; easing.type: Easing.InQuad }
                PropertyAction { value: 0 }
            }
        }
    }
}
