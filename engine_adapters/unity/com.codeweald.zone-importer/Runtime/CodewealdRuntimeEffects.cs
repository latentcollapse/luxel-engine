using System.Collections.Generic;
using UnityEngine;

namespace Codeweald.ZoneImporter
{
    /// <summary>Runtime translation of the portable objective_pulse contract.</summary>
    public sealed class CodewealdObjectivePulse : MonoBehaviour
    {
        public float PeriodSeconds = 2.4f;
        public float MinimumEnergy = 1.4f;
        public float MaximumEnergy = 4.2f;
        public Color Color = new Color(0.46f, 0.05f, 0.90f);
        private Light _light;

        private void Awake()
        {
            _light = GetComponent<Light>() ?? gameObject.AddComponent<Light>();
            _light.type = LightType.Point;
            _light.range = 42f;
            _light.color = Color;
        }

        private void Update()
        {
            var phase = (Mathf.Sin(Time.time * Mathf.PI * 2f / Mathf.Max(PeriodSeconds, 0.1f)) + 1f) * 0.5f;
            _light.intensity = Mathf.Lerp(MinimumEnergy, MaximumEnergy, phase);
        }
    }

    /// <summary>Small runtime treatment for a generated stream line; terrain water remains authored by the adapter.</summary>
    [RequireComponent(typeof(LineRenderer))]
    public sealed class CodewealdWaterFlow : MonoBehaviour
    {
        public float FlowSpeed = 0.65f;
        public float WaveAmplitude = 0.10f;
        private LineRenderer _line;

        private void Awake()
        {
            _line = GetComponent<LineRenderer>();
            _line.material = new Material(Shader.Find("Sprites/Default"));
            _line.startColor = _line.endColor = new Color(0.05f, 0.22f, 0.32f, 0.9f);
        }

        private void Update()
        {
            _line.widthMultiplier = Mathf.Max(0.1f, 14f + Mathf.Sin(Time.time * FlowSpeed * 6f) * WaveAmplitude * 8f);
        }
    }

    /// <summary>Deterministically sways direct instantiated foliage roots around their own bases.</summary>
    public sealed class CodewealdFoliageWind : MonoBehaviour
    {
        public float GustPeriodSeconds = 7f;
        public float SwayDegrees = 3f;
        private readonly List<Transform> _targets = new List<Transform>();
        private readonly List<Quaternion> _baseRotations = new List<Quaternion>();

        private void Start()
        {
            foreach (Transform child in transform)
            {
                _targets.Add(child);
                _baseRotations.Add(child.localRotation);
            }
        }

        private void Update()
        {
            var frequency = Mathf.PI * 2f / Mathf.Max(GustPeriodSeconds, 0.1f);
            for (var i = 0; i < _targets.Count; i++)
            {
                if (_targets[i] == null) continue;
                var phase = Time.time * frequency + i * 1.618f;
                _targets[i].localRotation = _baseRotations[i] * Quaternion.Euler(Mathf.Sin(phase) * SwayDegrees, 0f, Mathf.Cos(phase * 1.17f) * SwayDegrees * 0.55f);
            }
        }
    }

    /// <summary>Simple keep-banner proxy for the portable banner_wave contract.</summary>
    public sealed class CodewealdBannerWave : MonoBehaviour
    {
        public float PeriodSeconds = 3.6f;
        public float AmplitudeDegrees = 7f;
        private Quaternion _baseRotation;

        private void Start() { _baseRotation = transform.localRotation; }

        private void Update()
        {
            var phase = Mathf.Sin(Time.time * Mathf.PI * 2f / Mathf.Max(PeriodSeconds, 0.1f));
            transform.localRotation = _baseRotation * Quaternion.Euler(0f, 0f, phase * AmplitudeDegrees);
        }
    }
}
