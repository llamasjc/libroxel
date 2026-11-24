package com.libroxel.app;

import android.content.Context;
import android.os.Build;
import android.os.Bundle;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.view.WindowManager;

import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import com.google.androidgamesdk.GameActivity;

/**
 * Libroxel's window: the game fills the whole screen, under the notch too,
 * and the status and navigation bars stay hidden as in other games. A swipe
 * in from the edge shows them for a moment.
 */
public class MainActivity extends GameActivity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        WindowCompat.setDecorFitsSystemWindows(getWindow(), false);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            WindowManager.LayoutParams attributes = getWindow().getAttributes();
            attributes.layoutInDisplayCutoutMode =
                    WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            getWindow().setAttributes(attributes);
        }
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        hideSystemBars();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            hideSystemBars();
        }
    }

    /**
     * Runs the vibration motor at {@code strength}, from 0 (off) to 1, until
     * it is set again, for HapticService:SetMotor in games. Called from
     * Libroxel's native code.
     */
    public void setMotor(float strength) {
        Vibrator vibrator = (Vibrator) getSystemService(Context.VIBRATOR_SERVICE);
        if (vibrator == null || !vibrator.hasVibrator()) {
            return;
        }
        if (strength <= 0f) {
            vibrator.cancel();
            return;
        }
        int amplitude = VibrationEffect.DEFAULT_AMPLITUDE;
        if (vibrator.hasAmplitudeControl()) {
            amplitude = Math.max(1, Math.min(255, Math.round(strength * 255f)));
        }
        // A motor runs until it is set back to 0; ten seconds at most, so a
        // game that forgets doesn't buzz on and on.
        vibrator.vibrate(VibrationEffect.createOneShot(10_000L, amplitude));
    }

    private void hideSystemBars() {
        WindowInsetsControllerCompat controller =
                WindowCompat.getInsetsController(getWindow(), getWindow().getDecorView());
        controller.setSystemBarsBehavior(
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
        controller.hide(WindowInsetsCompat.Type.systemBars());
    }
}
