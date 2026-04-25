// landing-bg.js

(function () {
  var root = document.documentElement;
  var landing = document.getElementById('landingPage');
  var langToggleBtn = document.getElementById('langToggle');
  var reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  var currentLang = localStorage.getItem('gaggle_lang') || 'zh';

  function setToggleLabel() {
    if (!langToggleBtn) return;
    langToggleBtn.textContent = currentLang === 'zh' ? 'EN' : '中文';
  }

  function updateLanguage() {
    document.documentElement.lang = currentLang === 'zh' ? 'zh-CN' : 'en';
    document.querySelectorAll('[data-zh][data-en]').forEach(function (element) {
      var value = element.getAttribute('data-' + currentLang);
      if (element.tagName === 'INPUT' || element.tagName === 'TEXTAREA') {
        element.placeholder = value;
      } else {
        element.innerHTML = value;
      }
    });
    setToggleLabel();
    localStorage.setItem('gaggle_lang', currentLang);
  }

  if (langToggleBtn) {
    langToggleBtn.addEventListener('click', function () {
      currentLang = currentLang === 'zh' ? 'en' : 'zh';
      updateLanguage();
    });
  }

  updateLanguage();

  if (landing) {
    var rafLocked = false;
    window.addEventListener('mousemove', function (event) {
      if (rafLocked) return;
      rafLocked = true;
      requestAnimationFrame(function () {
        root.style.setProperty('--mouse-x', event.clientX + 'px');
        root.style.setProperty('--mouse-y', event.clientY + 'px');
        rafLocked = false;
      });
    });
  }

  var revealItems = document.querySelectorAll('.landing-reveal');
  if (revealItems.length) {
    if (reduceMotion || !('IntersectionObserver' in window)) {
      revealItems.forEach(function (item) { item.classList.add('is-visible'); });
    } else {
      var observer = new IntersectionObserver(function (entries) {
        entries.forEach(function (entry) {
          if (entry.isIntersecting) {
            entry.target.classList.add('is-visible');
            observer.unobserve(entry.target);
          }
        });
      }, { threshold: 0.14 });

      revealItems.forEach(function (item, index) {
        item.style.transitionDelay = Math.min(index * 100, 500) + 'ms';
        observer.observe(item);
      });
    }
  }

  var canvas = document.getElementById('heroCanvas');
  if (!canvas || reduceMotion || !landing) return;

  var ctx = canvas.getContext('2d');
  if (!ctx) return;

  var width = 0;
  var height = 0;
  var dpr = Math.min(window.devicePixelRatio || 1, 2);
  var particles = [];
  var mouse = { x: window.innerWidth / 2, y: window.innerHeight / 2, active: false };
  var heroAnchor = { x: window.innerWidth / 2, y: window.innerHeight * 0.28 };

  function getCounts() {
    if (window.innerWidth < 768) return { base: 68, hero: 12, maxDistance: 110 };
    return { base: 96, hero: 18, maxDistance: 150 };
  }

  function createParticle(isHero) {
    var targetX = heroAnchor.x + (Math.random() - 0.5) * 220;
    var targetY = heroAnchor.y + (Math.random() - 0.5) * 140;

    return {
      x: isHero ? targetX : Math.random() * width,
      y: isHero ? targetY : Math.random() * height,
      vx: (Math.random() - 0.5) * 0.22,
      vy: (Math.random() - 0.5) * 0.22,
      radius: isHero ? 2 + Math.random() * 2 : 0.5 + Math.random() * 1.5,
      alpha: isHero ? 0.32 + Math.random() * 0.08 : 0.15 + Math.random() * 0.25,
      hero: isHero
    };
  }

  function resize() {
    width = window.innerWidth;
    height = Math.max(window.innerHeight, landing.offsetHeight);
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.floor(width * dpr);
    canvas.height = Math.floor(height * dpr);
    canvas.style.width = width + 'px';
    canvas.style.height = height + 'px';
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    var heroTitle = document.querySelector('.hero-title');
    if (heroTitle) {
      var rect = heroTitle.getBoundingClientRect();
      heroAnchor.x = rect.left + rect.width / 2;
      heroAnchor.y = rect.top + rect.height / 2 + window.scrollY;
    } else {
      heroAnchor.x = width / 2;
      heroAnchor.y = height * 0.25;
    }

    var counts = getCounts();
    particles = [];
    for (var i = 0; i < counts.base; i++) particles.push(createParticle(false));
    for (var j = 0; j < counts.hero; j++) particles.push(createParticle(true));
  }

  function updateParticle(particle) {
    var scrollInfluence = window.scrollY * 0.00008;
    particle.x += particle.vx;
    particle.y += particle.vy + scrollInfluence;

    if (mouse.active) {
      var dx = particle.x - mouse.x;
      var dy = particle.y - mouse.y;
      var distance = Math.sqrt(dx * dx + dy * dy) || 1;
      if (distance < 200) {
        var force = (200 - distance) / 200;
        particle.x += (dx / distance) * force * (particle.hero ? 1.8 : 1.2);
        particle.y += (dy / distance) * force * (particle.hero ? 1.8 : 1.2);
      }
    }

    if (particle.x < -20) particle.x = width + 20;
    if (particle.x > width + 20) particle.x = -20;
    if (particle.y < -20) particle.y = height + 20;
    if (particle.y > height + 20) particle.y = -20;
  }

  function drawParticle(particle) {
    if (particle.hero) {
      ctx.shadowBlur = 6;
      ctx.shadowColor = 'rgba(200,200,255,0.25)';
    } else {
      ctx.shadowBlur = 0;
    }

    ctx.beginPath();
    ctx.arc(particle.x, particle.y, particle.radius, 0, Math.PI * 2);
    ctx.fillStyle = 'rgba(255,255,255,' + particle.alpha + ')';
    ctx.fill();
    ctx.shadowBlur = 0;
  }

  function drawConnections(maxDistance) {
    for (var i = 0; i < particles.length; i++) {
      for (var j = i + 1; j < particles.length; j++) {
        var dx = particles[i].x - particles[j].x;
        var dy = particles[i].y - particles[j].y;
        var distance = Math.sqrt(dx * dx + dy * dy);
        if (distance >= maxDistance) continue;
        var opacity = (1 - distance / maxDistance) * 0.12;
        ctx.beginPath();
        ctx.moveTo(particles[i].x, particles[i].y);
        ctx.lineTo(particles[j].x, particles[j].y);
        ctx.strokeStyle = 'rgba(255,255,255,' + opacity.toFixed(3) + ')';
        ctx.lineWidth = particles[i].hero || particles[j].hero ? 1 : 0.7;
        ctx.stroke();
      }
    }
  }

  function animate() {
    if (landing.style.display === 'none') {
      requestAnimationFrame(animate);
      return;
    }

    var counts = getCounts();
    ctx.clearRect(0, 0, width, height);
    for (var i = 0; i < particles.length; i++) {
      updateParticle(particles[i]);
      drawParticle(particles[i]);
    }
    drawConnections(counts.maxDistance);
    requestAnimationFrame(animate);
  }

  window.addEventListener('mousemove', function (event) {
    mouse.x = event.clientX;
    mouse.y = event.clientY + window.scrollY;
    mouse.active = true;
  });

  window.addEventListener('mouseleave', function () {
    mouse.active = false;
  });

  window.addEventListener('resize', resize);
  resize();
  animate();
})();
