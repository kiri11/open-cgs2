import QtQuick 2.14

Canvas {
    id: icon
    property string condition
    property bool night: false

    readonly property string kind: {
        switch (condition) {
        case "sunny": return night ? "moon" : "sun";
        case "clear-night": return "moon";
        case "partlycloudy": return night ? "moon-cloud" : "sun-cloud";
        case "cloudy": return "cloud";
        case "rainy": return "rain";
        case "pouring": return "pour";
        case "lightning": case "lightning-rainy": case "exceptional": return "storm";
        case "snowy": case "hail": return "snow";
        case "snowy-rainy": return "sleet";
        case "fog": return "fog";
        case "windy": case "windy-variant": return "wind";
        case "drop": return "drop";
        default: return "";
        }
    }

    onKindChanged: requestPaint()
    onVisibleChanged: if (visible) requestPaint()

    onPaint: {
        var ctx = getContext("2d");
        var w = width, h = height;
        ctx.reset();
        ctx.lineCap = "round";
        var sunColor = "#f7c548", moonColor = "#d9dde6", cloudColor = "#c3cbd6", dark = "#8f99a8", blue = "#3ec6ff";

        function sun(cx, cy, r) {
            ctx.fillStyle = sunColor;
            ctx.strokeStyle = sunColor;
            ctx.lineWidth = r * 0.32;
            ctx.beginPath();
            ctx.arc(cx, cy, r, 0, Math.PI * 2);
            ctx.fill();
            ctx.beginPath();
            for (var i = 0; i < 8; i++) {
                var a = i * Math.PI / 4;
                ctx.moveTo(cx + Math.cos(a) * r * 1.45, cy + Math.sin(a) * r * 1.45);
                ctx.lineTo(cx + Math.cos(a) * r * 1.9, cy + Math.sin(a) * r * 1.9);
            }
            ctx.stroke();
        }
        function moon(cx, cy, r) {
            ctx.fillStyle = moonColor;
            ctx.beginPath();
            ctx.arc(cx, cy, r, 0, Math.PI * 2);
            ctx.fill();
            ctx.globalCompositeOperation = "destination-out";
            ctx.beginPath();
            ctx.arc(cx + r * 0.55, cy - r * 0.35, r * 0.85, 0, Math.PI * 2);
            ctx.fill();
            ctx.globalCompositeOperation = "source-over";
        }
        function cloud(cx, cy, s, color) {
            ctx.fillStyle = color;
            [[-0.25, 0.06, 0.2], [0.02, -0.06, 0.27], [0.27, 0.08, 0.18]].forEach(function(c) {
                ctx.beginPath();
                ctx.arc(cx + s * c[0], cy + s * c[1], s * c[2], 0, Math.PI * 2);
                ctx.fill();
            });
            ctx.fillRect(cx - s * 0.25, cy + s * 0.06, s * 0.52, s * 0.2);
        }
        function drops(n, len, color) {
            ctx.strokeStyle = color;
            ctx.lineWidth = w * 0.07;
            ctx.beginPath();
            for (var i = 0; i < n; i++) {
                var x = w * (0.5 + (i - (n - 1) / 2) * 0.17);
                ctx.moveTo(x, h * 0.7);
                ctx.lineTo(x - w * 0.05 * len, h * (0.7 + 0.16 * len));
            }
            ctx.stroke();
        }
        function flakes(n) {
            ctx.fillStyle = "#eef2f6";
            for (var i = 0; i < n; i++) {
                ctx.beginPath();
                ctx.arc(w * (0.5 + (i - (n - 1) / 2) * 0.18), h * (i % 2 ? 0.86 : 0.76), w * 0.045, 0, Math.PI * 2);
                ctx.fill();
            }
        }
        function lines(rows) {
            ctx.strokeStyle = cloudColor;
            ctx.lineWidth = w * 0.07;
            ctx.beginPath();
            for (var i = 0; i < rows.length; i++) {
                ctx.moveTo(w * rows[i][0], h * rows[i][2]);
                ctx.lineTo(w * rows[i][1], h * rows[i][2]);
            }
            ctx.stroke();
        }

        var k = kind;
        if (k === "sun") {
            sun(w / 2, h / 2, w * 0.2);
        } else if (k === "moon") {
            moon(w / 2, h / 2, w * 0.3);
        } else if (k === "sun-cloud") {
            sun(w * 0.38, h * 0.36, w * 0.15);
            cloud(w * 0.55, h * 0.58, w * 0.72, cloudColor);
        } else if (k === "moon-cloud") {
            moon(w * 0.38, h * 0.34, w * 0.2);
            cloud(w * 0.55, h * 0.58, w * 0.72, cloudColor);
        } else if (k === "cloud") {
            cloud(w * 0.5, h * 0.48, w * 0.9, cloudColor);
        } else if (k === "rain" || k === "pour" || k === "sleet" || k === "snow" || k === "storm") {
            cloud(w * 0.5, h * 0.36, w * 0.86, k === "storm" || k === "pour" ? dark : cloudColor);
            if (k === "rain")
                drops(3, 1, blue);
            else if (k === "pour")
                drops(4, 1.4, blue);
            else if (k === "snow")
                flakes(3);
            else if (k === "sleet") {
                drops(2, 1, blue);
                flakes(1);
            } else {
                ctx.fillStyle = sunColor;
                ctx.beginPath();
                ctx.moveTo(w * 0.54, h * 0.6);
                ctx.lineTo(w * 0.38, h * 0.8);
                ctx.lineTo(w * 0.5, h * 0.8);
                ctx.lineTo(w * 0.44, h * 0.97);
                ctx.lineTo(w * 0.64, h * 0.74);
                ctx.lineTo(w * 0.52, h * 0.74);
                ctx.closePath();
                ctx.fill();
            }
        } else if (k === "fog") {
            lines([[0.18, 0.82, 0.3], [0.1, 0.74, 0.5], [0.26, 0.9, 0.7]]);
        } else if (k === "wind") {
            lines([[0.12, 0.7, 0.32], [0.2, 0.88, 0.52], [0.12, 0.6, 0.72]]);
        } else if (k === "drop") {
            ctx.fillStyle = blue;
            ctx.beginPath();
            ctx.moveTo(w / 2, h * 0.06);
            ctx.bezierCurveTo(w * 0.62, h * 0.3, w * 0.86, h * 0.5, w * 0.86, h * 0.66);
            ctx.bezierCurveTo(w * 0.86, h * 0.86, w * 0.7, h * 0.96, w / 2, h * 0.96);
            ctx.bezierCurveTo(w * 0.3, h * 0.96, w * 0.14, h * 0.86, w * 0.14, h * 0.66);
            ctx.bezierCurveTo(w * 0.14, h * 0.5, w * 0.38, h * 0.3, w / 2, h * 0.06);
            ctx.fill();
        }
    }
}
