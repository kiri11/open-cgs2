import QtQuick 2.14
import "../../qml"

Canvas {
    onPaint: {
        var ctx = getContext("2d");
        var w = width, h = height;
        ctx.reset();
        ctx.fillStyle = "#1ed760";
        ctx.beginPath();
        ctx.arc(w / 2, h / 2, w / 2, 0, Math.PI * 2);
        ctx.fill();
        ctx.strokeStyle = "#121212";
        ctx.lineCap = "round";
        for (var i = 0; i < 3; i++) {
            var y = h * (0.37 + i * 0.165);
            ctx.lineWidth = w * (0.095 - i * 0.017);
            ctx.beginPath();
            ctx.moveTo(w * (0.25 + i * 0.03), y);
            ctx.quadraticCurveTo(w * 0.5, y - h * 0.09, w * (0.75 - i * 0.04), y + h * 0.06);
            ctx.stroke();
        }
    }
}
