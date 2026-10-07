import QtQuick 2.14

Canvas {
    id: icon
    property string kind
    property color color: "white"

    onKindChanged: requestPaint()
    onColorChanged: requestPaint()

    onPaint: {
        var ctx = getContext("2d");
        var w = width, h = height;
        ctx.reset();
        ctx.fillStyle = color;
        ctx.strokeStyle = color;
        ctx.lineCap = "round";
        ctx.lineWidth = w * 0.1;
        function tri(x0, x1) {
            ctx.beginPath();
            ctx.moveTo(x0, h * 0.2);
            ctx.lineTo(x1, h / 2);
            ctx.lineTo(x0, h * 0.8);
            ctx.closePath();
            ctx.fill();
        }
        if (kind === "play") {
            tri(w * 0.3, w * 0.8);
        } else if (kind === "pause") {
            ctx.fillRect(w * 0.26, h * 0.2, w * 0.17, h * 0.6);
            ctx.fillRect(w * 0.57, h * 0.2, w * 0.17, h * 0.6);
        } else if (kind === "next") {
            tri(w * 0.2, w * 0.68);
            ctx.fillRect(w * 0.68, h * 0.2, w * 0.1, h * 0.6);
        } else if (kind === "prev") {
            ctx.fillRect(w * 0.22, h * 0.2, w * 0.1, h * 0.6);
            ctx.beginPath();
            ctx.moveTo(w * 0.8, h * 0.2);
            ctx.lineTo(w * 0.32, h / 2);
            ctx.lineTo(w * 0.8, h * 0.8);
            ctx.closePath();
            ctx.fill();
        } else if (kind === "mic") {
            ctx.beginPath();
            ctx.arc(w * 0.5, h * 0.22, w * 0.2, Math.PI, 0);
            ctx.lineTo(w * 0.7, h * 0.52);
            ctx.arc(w * 0.5, h * 0.52, w * 0.2, 0, Math.PI);
            ctx.closePath();
            ctx.fill();
            ctx.beginPath();
            ctx.arc(w * 0.5, h * 0.5, w * 0.38, 0, Math.PI);
            ctx.moveTo(w * 0.5, h * 0.5 + w * 0.38);
            ctx.lineTo(w * 0.5, h * 0.92);
            ctx.moveTo(w * 0.32, h * 0.92);
            ctx.lineTo(w * 0.68, h * 0.92);
            ctx.stroke();
        } else if (kind === "power") {
            ctx.beginPath();
            ctx.arc(w / 2, h * 0.55, w * 0.33, -Math.PI / 2 + 0.7, Math.PI * 1.5 - 0.7);
            ctx.moveTo(w / 2, h * 0.1);
            ctx.lineTo(w / 2, h * 0.48);
            ctx.stroke();
        } else if (kind === "down") {
            ctx.beginPath();
            ctx.moveTo(w * 0.2, h * 0.36);
            ctx.lineTo(w / 2, h * 0.66);
            ctx.lineTo(w * 0.8, h * 0.36);
            ctx.stroke();
        } else if (kind === "plus" || kind === "minus") {
            ctx.beginPath();
            ctx.moveTo(w * 0.22, h / 2);
            ctx.lineTo(w * 0.78, h / 2);
            if (kind === "plus") {
                ctx.moveTo(w / 2, h * 0.22);
                ctx.lineTo(w / 2, h * 0.78);
            }
            ctx.stroke();
        } else if (kind === "flame") {
            ctx.lineJoin = "round";
            ctx.beginPath();
            ctx.moveTo(w * 0.5, h * 0.08);
            ctx.quadraticCurveTo(w * 0.86, h * 0.4, w * 0.8, h * 0.66);
            ctx.bezierCurveTo(w * 0.76, h * 0.86, w * 0.6, h * 0.92, w * 0.5, h * 0.92);
            ctx.bezierCurveTo(w * 0.4, h * 0.92, w * 0.24, h * 0.86, w * 0.2, h * 0.66);
            ctx.quadraticCurveTo(w * 0.16, h * 0.46, w * 0.34, h * 0.3);
            ctx.quadraticCurveTo(w * 0.36, h * 0.46, w * 0.45, h * 0.5);
            ctx.quadraticCurveTo(w * 0.58, h * 0.3, w * 0.5, h * 0.08);
            ctx.stroke();
        } else if (kind === "snow") {
            ctx.lineWidth = w * 0.08;
            ctx.beginPath();
            for (var i = 0; i < 6; i++) {
                var a = i * Math.PI / 3 - Math.PI / 2, ca = Math.cos(a), sa = Math.sin(a);
                var bx = w / 2 + ca * w * 0.24, by = h / 2 + sa * h * 0.24;
                ctx.moveTo(w / 2, h / 2);
                ctx.lineTo(w / 2 + ca * w * 0.42, h / 2 + sa * h * 0.42);
                ctx.moveTo(bx + Math.cos(a - 0.9) * w * 0.12, by + Math.sin(a - 0.9) * h * 0.12);
                ctx.lineTo(bx, by);
                ctx.lineTo(bx + Math.cos(a + 0.9) * w * 0.12, by + Math.sin(a + 0.9) * h * 0.12);
            }
            ctx.stroke();
        } else if (kind === "fan") {
            ctx.lineJoin = "round";
            for (var b = 0; b < 3; b++) {
                ctx.save();
                ctx.translate(w / 2, h / 2);
                ctx.rotate(b * Math.PI * 2 / 3);
                ctx.beginPath();
                ctx.moveTo(0, 0);
                ctx.bezierCurveTo(-w * 0.2, -h * 0.16, -w * 0.12, -h * 0.42, w * 0.06, -h * 0.4);
                ctx.bezierCurveTo(w * 0.2, -h * 0.38, w * 0.16, -h * 0.16, 0, 0);
                ctx.stroke();
                ctx.restore();
            }
        } else if (kind === "close") {
            ctx.beginPath();
            ctx.moveTo(w * 0.25, h * 0.25);
            ctx.lineTo(w * 0.75, h * 0.75);
            ctx.moveTo(w * 0.75, h * 0.25);
            ctx.lineTo(w * 0.25, h * 0.75);
            ctx.stroke();
        } else if (kind === "shift") {
            ctx.lineWidth = w * 0.08;
            ctx.lineJoin = "round";
            ctx.beginPath();
            ctx.moveTo(w * 0.5, h * 0.15);
            ctx.lineTo(w * 0.85, h * 0.52);
            ctx.lineTo(w * 0.65, h * 0.52);
            ctx.lineTo(w * 0.65, h * 0.85);
            ctx.lineTo(w * 0.35, h * 0.85);
            ctx.lineTo(w * 0.35, h * 0.52);
            ctx.lineTo(w * 0.15, h * 0.52);
            ctx.closePath();
            ctx.stroke();
        } else if (kind === "erase") {
            ctx.lineWidth = w * 0.08;
            ctx.lineJoin = "round";
            ctx.beginPath();
            ctx.moveTo(w * 0.08, h * 0.5);
            ctx.lineTo(w * 0.32, h * 0.22);
            ctx.lineTo(w * 0.92, h * 0.22);
            ctx.lineTo(w * 0.92, h * 0.78);
            ctx.lineTo(w * 0.32, h * 0.78);
            ctx.closePath();
            ctx.moveTo(w * 0.48, h * 0.38);
            ctx.lineTo(w * 0.72, h * 0.62);
            ctx.moveTo(w * 0.72, h * 0.38);
            ctx.lineTo(w * 0.48, h * 0.62);
            ctx.stroke();
        } else if (kind === "bell") {
            ctx.lineWidth = w * 0.08;
            ctx.lineJoin = "round";
            ctx.beginPath();
            ctx.moveTo(w * 0.16, h * 0.74);
            ctx.bezierCurveTo(w * 0.28, h * 0.62, w * 0.26, h * 0.52, w * 0.28, h * 0.42);
            ctx.bezierCurveTo(w * 0.3, h * 0.1, w * 0.7, h * 0.1, w * 0.72, h * 0.42);
            ctx.bezierCurveTo(w * 0.74, h * 0.52, w * 0.72, h * 0.62, w * 0.84, h * 0.74);
            ctx.closePath();
            ctx.stroke();
            ctx.beginPath();
            ctx.arc(w * 0.5, h * 0.84, w * 0.08, 0, Math.PI * 2);
            ctx.fill();
        } else if (kind === "timer") {
            ctx.lineWidth = w * 0.08;
            ctx.beginPath();
            ctx.arc(w * 0.5, h * 0.57, w * 0.33, 0, Math.PI * 2);
            ctx.moveTo(w * 0.4, h * 0.08);
            ctx.lineTo(w * 0.6, h * 0.08);
            ctx.moveTo(w * 0.5, h * 0.08);
            ctx.lineTo(w * 0.5, h * 0.24);
            ctx.moveTo(w * 0.5, h * 0.57);
            ctx.lineTo(w * 0.5, h * 0.38);
            ctx.stroke();
        } else if (kind === "back") {
            ctx.beginPath();
            ctx.moveTo(w * 0.62, h * 0.2);
            ctx.lineTo(w * 0.32, h * 0.5);
            ctx.lineTo(w * 0.62, h * 0.8);
            ctx.stroke();
        } else if (kind === "shuffle") {
            ctx.lineWidth = w * 0.08;
            ctx.lineJoin = "round";
            ctx.beginPath();
            ctx.moveTo(w * 0.12, h * 0.3);
            ctx.lineTo(w * 0.28, h * 0.3);
            ctx.bezierCurveTo(w * 0.5, h * 0.3, w * 0.5, h * 0.7, w * 0.72, h * 0.7);
            ctx.lineTo(w * 0.86, h * 0.7);
            ctx.moveTo(w * 0.12, h * 0.7);
            ctx.lineTo(w * 0.28, h * 0.7);
            ctx.bezierCurveTo(w * 0.5, h * 0.7, w * 0.5, h * 0.3, w * 0.72, h * 0.3);
            ctx.lineTo(w * 0.86, h * 0.3);
            ctx.moveTo(w * 0.74, h * 0.18);
            ctx.lineTo(w * 0.88, h * 0.3);
            ctx.lineTo(w * 0.74, h * 0.42);
            ctx.moveTo(w * 0.74, h * 0.58);
            ctx.lineTo(w * 0.88, h * 0.7);
            ctx.lineTo(w * 0.74, h * 0.82);
            ctx.stroke();
        } else if (kind === "lock") {
            ctx.lineWidth = w * 0.09;
            ctx.beginPath();
            ctx.arc(w * 0.5, h * 0.42, w * 0.2, Math.PI, 0);
            ctx.lineTo(w * 0.7, h * 0.48);
            ctx.moveTo(w * 0.3, h * 0.48);
            ctx.lineTo(w * 0.3, h * 0.42);
            ctx.stroke();
            ctx.fillRect(w * 0.2, h * 0.48, w * 0.6, h * 0.42);
        } else if (kind === "calendar") {
            ctx.lineWidth = w * 0.08;
            ctx.lineJoin = "round";
            ctx.strokeRect(w * 0.14, h * 0.2, w * 0.72, h * 0.68);
            ctx.fillRect(w * 0.14, h * 0.2, w * 0.72, h * 0.18);
            ctx.beginPath();
            ctx.moveTo(w * 0.34, h * 0.08);
            ctx.lineTo(w * 0.34, h * 0.26);
            ctx.moveTo(w * 0.66, h * 0.08);
            ctx.lineTo(w * 0.66, h * 0.26);
            ctx.stroke();
        } else if (kind === "video") {
            ctx.lineWidth = w * 0.08;
            ctx.lineJoin = "round";
            ctx.strokeRect(w * 0.1, h * 0.28, w * 0.54, h * 0.44);
            ctx.beginPath();
            ctx.moveTo(w * 0.64, h * 0.44);
            ctx.lineTo(w * 0.9, h * 0.3);
            ctx.lineTo(w * 0.9, h * 0.7);
            ctx.lineTo(w * 0.64, h * 0.56);
            ctx.closePath();
            ctx.fill();
        }
    }
}
